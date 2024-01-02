use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request, TimeOrNow,
};
use libc::{c_int, EEXIST, EINVAL, ENOENT, ENOTEMPTY};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    iter,
    time::{Duration, SystemTime},
};
use sysinfo::System;

type Ino = u64;

#[derive(Debug, Clone)]
pub struct Node {
    children: BTreeMap<String, Ino>,
    parent: Ino,
}

impl Node {
    fn new(parent: Ino) -> Node {
        Node {
            children: BTreeMap::new(),
            parent,
        }
    }
}

pub struct MemFS {
    files: BTreeMap<Ino, Vec<u8>>,
    attrs: BTreeMap<Ino, FileAttr>,
    tree: BTreeMap<Ino, Node>,
    next: Ino,
}

impl MemFS {
    pub fn new() -> MemFS {
        let files: BTreeMap<Ino, Vec<u8>> = BTreeMap::new();
        let mut attrs: BTreeMap<Ino, FileAttr> = BTreeMap::new();
        let mut tree: BTreeMap<Ino, Node> = BTreeMap::new();
        let ts: SystemTime = SystemTime::now();
        attrs.insert(
            1,
            FileAttr {
                ino: 1,
                size: 0,
                blocks: 0,
                atime: ts,
                mtime: ts,
                ctime: ts,
                crtime: ts,
                kind: FileType::Directory,
                perm: 0o777,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
        );
        tree.insert(1, Node::new(1 as Ino));
        MemFS {
            files,
            attrs,
            tree,
            next: 2,
        }
    }

    fn next_inode(&mut self) -> Ino {
        self.next += 1;
        self.next
    }

    pub fn get(&mut self, inode: Ino) -> Result<&FileAttr, c_int> {
        self.attrs.get(&inode).ok_or(ENOENT)
    }

    pub fn set(
        &mut self,
        inode: Ino,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        ctime: Option<SystemTime>,
        crtime: Option<SystemTime>,
    ) -> Result<&FileAttr, c_int> {
        let attr: &mut FileAttr = self.attrs.get_mut(&inode).ok_or(ENOENT)?;
        let ts: SystemTime = SystemTime::now();

        match size {
            Some(new_size) => {
                if new_size < attr.size {
                    self.files
                        .get_mut(&inode)
                        .ok_or(ENOENT)?
                        .truncate(new_size as usize);
                }
                attr.size = new_size;
            }
            _ => (),
        }

        atime.map(|new_atime: TimeOrNow| {
            attr.mtime = match new_atime {
                TimeOrNow::SpecificTime(time) => time,
                TimeOrNow::Now => ts,
            }
        });
        mtime.map(|new_mtime: TimeOrNow| {
            attr.mtime = match new_mtime {
                TimeOrNow::SpecificTime(time) => time,
                TimeOrNow::Now => ts,
            }
        });
        ctime.map(|new_ctime: SystemTime| attr.ctime = new_ctime);
        crtime.map(|new_crtime: SystemTime| attr.crtime = new_crtime);

        Ok(attr)
    }

    pub fn readdir(
        &mut self,
        inode: Ino,
        offset: i64,
    ) -> Result<Vec<(Ino, FileType, String)>, c_int> {
        let mut entries: Vec<(Ino, FileType, String)> = Vec::new();
        if offset == 0 {
            entries.push((inode, FileType::Directory, String::from(".")));
        };
        self.tree.get(&inode).map_or(Err(ENOENT), |ino: &Node| {
            if offset == 0 {
                entries.push((ino.parent, FileType::Directory, String::from("..")));
            }

            for (child_name, child_inode) in
                ino.children
                    .iter()
                    .skip(if offset > 0 { (offset - 1) as usize } else { 0 })
            {
                let child_attr: &&FileAttr = &self.attrs.get(child_inode).unwrap();
                entries.push((child_attr.ino, child_attr.kind, String::from(child_name)));
            }

            Ok(entries)
        })
    }

    pub fn lookup(&mut self, parent: Ino, name: &OsStr) -> Result<&FileAttr, c_int> {
        let inode: &Ino = self
            .tree
            .get(&parent)
            .ok_or(ENOENT)?
            .children
            .get(name.to_str().unwrap())
            .ok_or(ENOENT)?;
        self.attrs.get(inode).ok_or(ENOENT)
    }

    pub fn rmdir(&mut self, parent: Ino, name: &OsStr) -> Result<(), c_int> {
        let name_str: &str = name.to_str().unwrap();
        let inode: Ino = *self
            .tree
            .get(&parent)
            .ok_or(ENOENT)?
            .children
            .get(name_str)
            .ok_or(ENOENT)?;
        if self.tree.get(&inode).ok_or(ENOENT)?.children.is_empty() {
            self.attrs.remove(&inode);
            self.tree
                .get_mut(&parent)
                .ok_or(ENOENT)?
                .children
                .remove(name_str);
            self.tree.remove(&inode);
            Ok(())
        } else {
            Err(ENOTEMPTY)
        }
    }

    pub fn mkdir(&mut self, parent: Ino, name: &OsStr) -> Result<&FileAttr, c_int> {
        let name_str: &str = name.to_str().unwrap();
        let inode: Ino = self.next_inode();
        let parent_inode: &mut Node = self.tree.get_mut(&parent).ok_or(ENOENT)?;
        if !parent_inode.children.contains_key(name_str) {
            let ts: SystemTime = SystemTime::now();
            parent_inode.children.insert(name_str.to_string(), inode);
            self.attrs.insert(
                inode,
                FileAttr {
                    ino: inode,
                    size: 0,
                    blocks: 0,
                    atime: ts,
                    mtime: ts,
                    ctime: ts,
                    crtime: ts,
                    kind: FileType::Directory,
                    perm: 0o777,
                    nlink: 0,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                    blksize: 0,
                    flags: 0,
                },
            );
            self.tree.insert(inode, Node::new(parent));
            self.attrs.get(&inode).ok_or(EINVAL)
        } else {
            Err(EEXIST)
        }
    }

    pub fn unlink(&mut self, parent: Ino, name: &OsStr) -> Result<(), c_int> {
        let inode: Ino = self
            .tree
            .get_mut(&parent)
            .ok_or(ENOENT)?
            .children
            .remove(name.to_str().unwrap())
            .ok_or(ENOENT)?;
        let attr: FileAttr = self.attrs.remove(&inode).ok_or(EINVAL)?;
        if attr.kind == FileType::RegularFile {
            self.files.remove(&inode);
        }
        self.tree.remove(&inode);
        Ok(())
    }

    pub fn create(&mut self, parent: Ino, name: &OsStr) -> Result<&FileAttr, c_int> {
        let name_str: &str = name.to_str().unwrap();
        let inode: Ino = self.next_inode();
        let parent_inode: &mut Node = self.tree.get_mut(&parent).ok_or(ENOENT)?;
        match parent_inode.children.get_mut(name_str) {
            Some(inode) => self.attrs.get(&inode).ok_or(EINVAL),
            None => {
                let ts: SystemTime = SystemTime::now();
                self.attrs.insert(
                    inode,
                    FileAttr {
                        ino: inode,
                        size: 0,
                        blocks: 0,
                        atime: ts,
                        mtime: ts,
                        ctime: ts,
                        crtime: ts,
                        kind: FileType::RegularFile,
                        perm: 0o777,
                        nlink: 0,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                        blksize: 0,
                        flags: 0,
                    },
                );
                self.files.insert(inode, Vec::new());
                parent_inode.children.insert(name_str.to_string(), inode);
                self.tree.insert(inode, Node::new(parent));
                self.attrs.get(&inode).ok_or(EINVAL)
            }
        }
    }

    pub fn write(&mut self, inode: Ino, offset: i64, data: &[u8]) -> Result<u64, c_int> {
        let ts: SystemTime = SystemTime::now();
        let attr: &mut FileAttr = self.attrs.get_mut(&inode).ok_or(EINVAL)?;
        let memfile: &mut Vec<u8> = self.files.get_mut(&inode).ok_or(ENOENT)?;

        if memfile.len() <= offset as usize {
            memfile.extend(iter::repeat(0).take(offset as usize - memfile.len()));
        }

        if offset as usize + data.len() > memfile.len() {
            memfile.splice(offset as usize.., data.iter().cloned());
        } else {
            memfile.splice(
                offset as usize..offset as usize + data.len(),
                data.iter().cloned(),
            );
        }

        attr.atime = ts;
        attr.mtime = ts;
        attr.size = memfile.len() as u64;
        Ok(data.len() as u64)
    }

    pub fn read(&mut self, inode: Ino, offset: i64, size: u32) -> Result<&[u8], c_int> {
        let attr: &mut FileAttr = self.attrs.get_mut(&inode).ok_or(EINVAL)?;
        let memfile: &mut Vec<u8> = self.files.get_mut(&inode).ok_or(ENOENT)?;
        attr.atime = SystemTime::now();
        if memfile.len() < offset as usize {
            return Err(EINVAL);
        } else if memfile.len() < offset as usize + size as usize {
            Ok(&memfile[offset as usize..])
        } else {
            Ok(&memfile[offset as usize..(offset as usize + size as usize)])
        }
    }

    pub fn rename(
        &mut self,
        parent: Ino,
        name: &OsStr,
        new_parent: Ino,
        new_name: &OsStr,
    ) -> Result<(), c_int> {
        let child: Ino = {
            self.tree
                .get_mut(&parent)
                .ok_or(ENOENT)?
                .children
                .remove(name.to_str().unwrap())
                .ok_or(ENOENT)?
        };

        self.tree
            .get_mut(&new_parent)
            .ok_or(EINVAL)?
            .children
            .insert(new_name.to_str().unwrap().to_string(), child);

        Ok(())
    }

    pub fn size(&mut self, inode: Ino) -> Result<(u64, u64), c_int> {
        let mut size: u64 = 0;
        let mut file: u64 = 0;
        let attr: &mut FileAttr = self.attrs.get_mut(&inode).ok_or(EINVAL)?;
        if attr.kind == FileType::Directory {
            for child in self.tree.get(&inode).ok_or(EINVAL)?.children.values() {
                let child_attr: &mut FileAttr = self.attrs.get_mut(&child).ok_or(EINVAL)?;
                size += child_attr.size;
                file += 1;
            }
            file += 1;
        } else {
            size = attr.size;
            file += 1;
        }
        Ok((size, file))
    }
}

impl Default for MemFS {
    fn default() -> Self {
        MemFS::new()
    }
}

impl Filesystem for MemFS {
    fn getattr(&mut self, _: &Request, inode: u64, reply: ReplyAttr) {
        match self.get(inode) {
            Ok(attr) => reply.attr(&Duration::new(0, 0), attr),
            Err(e) => reply.error(e),
        }
    }

    fn setattr(
        &mut self,
        _: &Request,
        inode: u64,
        _: Option<u32>,
        _: Option<u32>,
        _: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        ctime: Option<SystemTime>,
        _: Option<u64>,
        crtime: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<u32>,
        reply: ReplyAttr,
    ) {
        match self.set(inode, size, atime, mtime, ctime, crtime) {
            Ok(attrs) => reply.attr(&Duration::new(0, 0), attrs),
            Err(e) => reply.error(e),
        };
    }

    fn readdir(&mut self, _: &Request, inode: u64, _: u64, offset: i64, mut reply: ReplyDirectory) {
        match self.readdir(inode, offset) {
            Ok(entries) => {
                for (i, entry) in entries.into_iter().enumerate() {
                    let _ = reply.add(entry.0, i as i64, entry.1, entry.2);
                }
                reply.ok();
            }
            Err(e) => reply.error(e),
        };
    }

    fn lookup(&mut self, _: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        match self.lookup(parent, name) {
            Ok(attr) => reply.entry(&Duration::new(0, 0), attr, 0),
            Err(e) => reply.error(e),
        }
    }

    fn rmdir(&mut self, _: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        match self.rmdir(parent, name) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn mkdir(&mut self, _: &Request, parent: u64, name: &OsStr, _: u32, _: u32, reply: ReplyEntry) {
        match self.mkdir(parent, name) {
            Ok(attr) => reply.entry(&Duration::new(0, 0), &attr, 0),
            Err(e) => reply.error(e),
        }
    }

    fn open(&mut self, _: &Request, _: u64, _: i32, reply: ReplyOpen) {
        reply.opened(0, 0);
    }

    fn unlink(&mut self, _: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        match self.unlink(parent, name) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn create(
        &mut self,
        _: &Request,
        parent: u64,
        name: &OsStr,
        _: u32,
        _: u32,
        _: i32,
        reply: ReplyCreate,
    ) {
        match self.create(parent, name) {
            Ok(attr) => reply.created(&Duration::new(0, 0), attr, 0, 0, 0),
            Err(e) => reply.error(e),
        }
    }

    fn write(
        &mut self,
        _: &Request,
        inode: u64,
        _: u64,
        offset: i64,
        data: &[u8],
        _: u32,
        _: i32,
        _: Option<u64>,
        reply: ReplyWrite,
    ) {
        match self.write(inode, offset, data) {
            Ok(bytes_written) => reply.written(bytes_written as u32),
            Err(e) => reply.error(e),
        }
    }

    fn read(
        &mut self,
        _: &Request,
        inode: u64,
        _: u64,
        offset: i64,
        size: u32,
        _: i32,
        _: Option<u64>,
        reply: ReplyData,
    ) {
        match self.read(inode, offset, size) {
            Ok(slice) => reply.data(slice),
            Err(e) => reply.error(e),
        }
    }

    fn rename(
        &mut self,
        _: &Request,
        parent: u64,
        name: &OsStr,
        new_parent: u64,
        new_name: &OsStr,
        _: u32,
        reply: ReplyEmpty,
    ) {
        match self.rename(parent, name, new_parent, new_name) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn statfs(&mut self, _: &Request, inode: u64, reply: ReplyStatfs) {
        let mut sys = System::new();
        sys.refresh_memory();

        match self.size(inode) {
            Ok((size, file)) => reply.statfs(size, 0, sys.free_memory(), file, 0, 1, 0, 0),
            Err(e) => reply.error(e),
        }
    }
}

fn main() {
    let options = vec![
        MountOption::FSName("memfs".to_string()),
        MountOption::AutoUnmount,
    ];
    let fs: MemFS = MemFS::new();
    let mountpoint: String = match std::env::args().nth(1) {
        Some(path) => path,
        _ => {
            print!("missing mountpoint");
            return;
        }
    };
    fuser::mount2(fs, &mountpoint, &options).unwrap();
}
