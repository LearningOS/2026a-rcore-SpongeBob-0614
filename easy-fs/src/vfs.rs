use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    inode_id: u32,
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    pub(crate) fn new(
        inode_id: u32,
        fs: Arc<Mutex<EasyFileSystem>>,
        fs_inner: &mut EasyFileSystem,
    ) -> Self {
        let (block_id, block_offset) = fs_inner.get_disk_inode_pos(inode_id);
        *fs_inner.inode_handles.entry(inode_id).or_insert(0) += 1;
        Self {
            inode_id,
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device: Arc::clone(&fs_inner.block_device),
        }
    }
    fn valid_name(name: &str) -> bool {
        !name.is_empty() && name.len() <= 27 && !name.bytes().any(|b| b == 0 || b == b'/')
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let mut fs = self.fs.lock();
        let inode_id = self.read_disk_inode(|disk_inode| self.find_inode_id(name, disk_inode))?;
        Some(Arc::new(Self::new(inode_id, self.fs.clone(), &mut fs)))
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            self.insert_dirent(name, new_inode_id, root_inode, &mut fs);
        });

        block_cache_sync_all();
        Some(Arc::new(Self::new(new_inode_id, self.fs.clone(), &mut fs)))
        // release efs lock automatically by compiler
    }

    fn insert_dirent(
        &self,
        name: &str,
        inode_id: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        let count = disk_inode.size as usize / DIRENT_SZ;
        let mut slot = count;
        let mut entry = DirEntry::empty();
        for index in 0..count {
            disk_inode.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
            if entry.name().is_empty() {
                slot = index;
                break;
            }
        }
        if slot == count {
            self.increase_size(((count + 1) * DIRENT_SZ) as u32, disk_inode, fs);
        }
        let entry = DirEntry::new(name, inode_id);
        disk_inode.write_at(slot * DIRENT_SZ, entry.as_bytes(), &self.block_device);
    }

    fn link_count(&self, inode_id: u32, fs: &EasyFileSystem) -> u32 {
        let (block_id, offset) = fs.get_disk_inode_pos(0);
        get_block_cache(block_id as usize, self.block_device.clone())
            .lock()
            .read(offset, |root: &DiskInode| {
                let mut count = 0;
                let mut entry = DirEntry::empty();
                for index in 0..root.size as usize / DIRENT_SZ {
                    root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                    if !entry.name().is_empty() && entry.inode_id() == inode_id {
                        count += 1;
                    }
                }
                count
            })
    }

    /// Return the inode number, directory flag and number of directory links.
    pub fn stat(&self) -> (u64, bool, u32) {
        let fs = self.fs.lock();
        let is_dir = self.read_disk_inode(|inode| inode.is_dir());
        let nlink = if self.inode_id == 0 {
            1
        } else {
            self.link_count(self.inode_id, &fs)
        };
        (self.inode_id as u64, is_dir, nlink)
    }

    /// Add a directory entry referring to an existing file.
    pub fn link(&self, old_name: &str, new_name: &str) -> bool {
        if !Self::valid_name(old_name) || !Self::valid_name(new_name) || old_name == new_name {
            return false;
        }
        let mut fs = self.fs.lock();
        let inode_id = self.read_disk_inode(|inode| {
            if self.find_inode_id(new_name, inode).is_some() {
                None
            } else {
                self.find_inode_id(old_name, inode)
            }
        });
        let inode_id = match inode_id {
            Some(id) => id,
            None => return false,
        };
        self.modify_disk_inode(|inode| self.insert_dirent(new_name, inode_id, inode, &mut fs));
        block_cache_sync_all();
        true
    }

    /// Remove a directory entry, reclaiming unreferenced files after their last handle closes.
    pub fn unlink(&self, name: &str) -> bool {
        if !Self::valid_name(name) {
            return false;
        }
        let mut fs = self.fs.lock();
        let inode_id = self.modify_disk_inode(|inode| {
            assert!(inode.is_dir());
            let mut entry = DirEntry::empty();
            for index in 0..inode.size as usize / DIRENT_SZ {
                inode.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if entry.name() == name {
                    let inode_id = entry.inode_id();
                    inode.write_at(
                        index * DIRENT_SZ,
                        DirEntry::empty().as_bytes(),
                        &self.block_device,
                    );
                    return Some(inode_id);
                }
            }
            None
        });
        let inode_id = match inode_id {
            Some(id) => id,
            None => return false,
        };
        if self.link_count(inode_id, &fs) == 0 {
            if fs.inode_handles.contains_key(&inode_id) {
                fs.unlinked_inodes.insert(inode_id);
            } else {
                fs.dealloc_inode(inode_id);
            }
        }
        block_cache_sync_all();
        true
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if !dirent.name().is_empty() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}

impl Drop for Inode {
    fn drop(&mut self) {
        let mut fs = self.fs.lock();
        let count = fs.inode_handles.get_mut(&self.inode_id).unwrap();
        *count -= 1;
        if *count == 0 {
            fs.inode_handles.remove(&self.inode_id);
            if fs.unlinked_inodes.remove(&self.inode_id) {
                fs.dealloc_inode(self.inode_id);
                block_cache_sync_all();
            }
        }
    }
}
