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
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
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
            if !dirent.name().is_empty() && dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
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
        if self
            .read_disk_inode(|root| self.find_inode_id(name, root))
            .is_some()
        {
            return None;
        }
        let id = fs.alloc_inode();
        let (block, offset) = fs.get_disk_inode_pos(id);
        get_block_cache(block as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(offset, |inode: &mut DiskInode| {
                inode.initialize(DiskInodeType::File)
            });
        self.insert_entry(name, id, &mut fs);
        block_cache_sync_all();
        Some(Arc::new(Self::new(
            block,
            offset,
            Arc::clone(&self.fs),
            Arc::clone(&self.block_device),
        )))
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

impl Inode {
    /// Stable on-disk inode number within this filesystem.
    pub fn inode_id(&self) -> u32 {
        self.fs
            .lock()
            .get_disk_inode_id(self.block_id as u32, self.block_offset)
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty() && name.len() <= 27 && !name.contains('/') && name != "." && name != ".."
    }

    // Caller holds the filesystem lock and has checked name uniqueness.
    fn insert_entry(&self, name: &str, id: u32, fs: &mut MutexGuard<EasyFileSystem>) {
        self.modify_disk_inode(|root| {
            let count = root.size as usize / DIRENT_SZ;
            let mut slot = count;
            for index in 0..count {
                let mut entry = DirEntry::empty();
                root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if entry.name().is_empty() {
                    slot = index;
                    break;
                }
            }
            if slot == count {
                self.increase_size(((count + 1) * DIRENT_SZ) as u32, root, fs);
            }
            root.write_at(
                slot * DIRENT_SZ,
                DirEntry::new(name, id).as_bytes(),
                &self.block_device,
            );
        });
    }

    /// Create another root-directory name for an existing regular inode.
    pub fn link(&self, old: &str, new: &str) -> bool {
        if !Self::valid_name(old) || !Self::valid_name(new) {
            return false;
        }
        let mut fs = self.fs.lock();
        let id = self.read_disk_inode(|root| {
            if self.find_inode_id(new, root).is_some() {
                None
            } else {
                self.find_inode_id(old, root)
            }
        });
        let Some(id) = id else {
            return false;
        };
        self.insert_entry(new, id, &mut fs);
        block_cache_sync_all();
        true
    }

    /// Remove one name, returning its inode for deferred reclamation by the OS.
    pub fn unlink(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let fs = self.fs.lock();
        let found = self.read_disk_inode(|root| {
            for index in 0..root.size as usize / DIRENT_SZ {
                let mut entry = DirEntry::empty();
                root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if entry.name() == name {
                    return Some((index, entry.inode_id()));
                }
            }
            None
        });
        let (index, id) = found?;
        self.modify_disk_inode(|root| {
            root.write_at(
                index * DIRENT_SZ,
                DirEntry::empty().as_bytes(),
                &self.block_device,
            );
        });
        let (block, offset) = fs.get_disk_inode_pos(id);
        block_cache_sync_all();
        Some(Arc::new(Self::new(
            block,
            offset,
            Arc::clone(&self.fs),
            Arc::clone(&self.block_device),
        )))
    }

    /// Count active root-directory links to an inode, excluding tombstones.
    pub fn link_count(&self, id: u32) -> u32 {
        let _fs = self.fs.lock();
        self.read_disk_inode(|root| {
            let mut count = 0;
            for index in 0..root.size as usize / DIRENT_SZ {
                let mut entry = DirEntry::empty();
                root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if !entry.name().is_empty() && entry.inode_id() == id {
                    count += 1;
                }
            }
            count
        })
    }

    /// Reclaim an inode after the caller proves it has no names or open handles.
    pub fn reclaim_unlinked(&self) {
        let mut fs = self.fs.lock();
        let id = fs.get_disk_inode_id(self.block_id as u32, self.block_offset);
        assert_ne!(id, 0, "cannot reclaim root inode");
        self.modify_disk_inode(|inode| {
            for block in inode.clear_size(&self.block_device) {
                fs.dealloc_data(block);
            }
        });
        fs.inode_bitmap.dealloc(&self.block_device, id as usize);
        block_cache_sync_all();
    }
}
