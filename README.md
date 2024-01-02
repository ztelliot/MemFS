# MemFS

基于 FUSE 实现的简易内存文件系统

## 实现的 FUSE 接口

- getattr
- setattr
- readdir
- lookup
- rmdir
- mkdir
- unlink
- create
- write
- read
- rename
- statfs


## 数据结构

```rust
type Ino = u64;

pub struct Node {
    children: BTreeMap<String, Ino>,
    parent: Ino,
}

pub struct MemFS {
    files: BTreeMap<Ino, Vec<u8>>,
    attrs: BTreeMap<Ino, fuser::FileAttr>,
    tree: BTreeMap<Ino, Node>,
    next: Ino,
}
```
