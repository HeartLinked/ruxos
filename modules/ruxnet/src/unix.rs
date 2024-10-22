/* Copyright (c) [2023] [Syswonder Community]
*   [Ruxos] is licensed under Mulan PSL v2.
*   You can use this software according to the terms and conditions of the Mulan PSL v2.
*   You may obtain a copy of Mulan PSL v2 at:
*               http://license.coscl.org.cn/MulanPSL2
*   THIS SOFTWARE IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OF ANY KIND, EITHER EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO NON-INFRINGEMENT, MERCHANTABILITY OR FIT FOR A PARTICULAR PURPOSE.
*   See the Mulan PSL v2 for more details.
*/

use alloc::{sync::Arc, vec, vec::Vec};
use smoltcp::socket::tcp::{SendError, RecvError};
use spin::RwLock;
use core::sync::atomic::AtomicIsize;
use core::ffi::{c_char, c_int, c_void};
use core::mem::size_of;
use core::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use axerrno::{LinuxError, LinuxResult, ax_err, ax_err_type, AxError, AxResult};
use axio::{PollState, Result};
use axsync::Mutex;

use lazy_init::LazyInit;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{self, AnySocket, tcp::SocketBuffer};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};

use flatten_objects::FlattenObjects;
use hashbrown::HashMap;

use ruxtask::yield_now;
use axfs_vfs::{VfsError, VfsNodeAttr, VfsNodeOps, VfsNodeRef, VfsNodeType, VfsOps, VfsResult};
use ruxfs::root::{lookup, create_file};

const SOCK_ADDR_UN_PATH_LEN: usize = 108;
const UNIX_SOCKET_BUFFER_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub struct SockaddrUn {
    pub sun_family: u16, /* AF_UNIX */
    /// if socket is unnamed, use `sun_path[0]` to save fd
    pub sun_path: [c_char; SOCK_ADDR_UN_PATH_LEN], /* Pathname */
}

impl SockaddrUn {
    pub fn set_addr(&mut self, new_addr :&SockaddrUn) {
        self.sun_family = new_addr.sun_family;
        self.sun_path = new_addr.sun_path;
    }
}

//To avoid owner question of FDTABLE outside and UnixTable in this crate we split the unixsocket
struct UnixSocketInner<'a> {
    pub addr: Mutex<SockaddrUn>,
    pub buf: SocketBuffer<'a>,
    pub peer_socket: Option<usize>,
    pub status: UnixSocketStatus,
}

impl<'a> UnixSocketInner<'a> {
    pub fn new() -> Self {
        Self {
            addr: Mutex::new(SockaddrUn {
                sun_family: 1, //AF_UNIX
                sun_path: [0; SOCK_ADDR_UN_PATH_LEN],
            }),
            buf: SocketBuffer::new(vec![0; 64*1024]),
            peer_socket: None,
            status: UnixSocketStatus::Closed,
        }
    }

    pub fn get_addr(&self) -> SockaddrUn {
        self.addr.lock().clone()
    }

    pub fn get_peersocket(&self) -> Option<usize> {
        self.peer_socket
    }

    pub fn set_peersocket(&mut self, peer: usize) {
        self.peer_socket = Some(peer)
    }

    pub fn get_state(&self) -> UnixSocketStatus{
        self.status
    }

    pub fn set_state(&mut self, state:UnixSocketStatus) {
        self.status = state
    }

}

/// unix domain socket.
pub struct UnixSocket {
    sockethandle: Option<usize>,
    unixsocket_type: UnixSocketType,
}

fn get_inode(addr: SockaddrUn) -> AxResult<usize>{
    let slice = unsafe {
        core::slice::from_raw_parts(addr.sun_path.as_ptr(), addr.sun_path.len())
    };

    let socket_path = unsafe {
        core::ffi::CStr::from_ptr(slice.as_ptr())
            .to_str()
            .expect("Invalid UTF-8 string")
    };
    let vfsnode = match lookup(None, socket_path) {
        Ok(node) => {
            node
        }
        Err(_) => {
            // lhw TODO socket type
            create_file(None, socket_path)?
        }
    };
    let metadata = vfsnode.get_attr()?;
    let st_ino = metadata.ino();
    Ok(st_ino as usize)
}

struct HashMapWarpper<'a> {
    inner:HashMap<usize, Arc<Mutex<UnixSocketInner<'a>>>>,
    index_allcator: usize,
}
impl<'a> HashMapWarpper<'a> {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            index_allcator:0,
        }
    }
    pub fn find<F>(&self, predicate: F) -> Option<(&usize, &Arc<Mutex<UnixSocketInner<'a>>>)>
    where
        F: Fn(&Arc<Mutex<UnixSocketInner<'_>>>) -> bool,
    {
        self.inner.iter().find(|(_k,v)|{predicate(v)})
    }
    
    pub fn add(&mut self, value: Arc<Mutex<UnixSocketInner<'a>>>) -> Option<usize> {
        while self.inner.contains_key(&self.index_allcator)
        {
            self.index_allcator += 1;
        }
        self.inner.insert(self.index_allcator ,value);
        Some(self.index_allcator)
    }

    pub fn replace_handle(&mut self, old: usize, new: usize) -> Option<usize> {
        if let Some(value) = self.inner.remove(&old) {
            self.inner.insert(new, value);
        }
        Some(new)
    }

    pub fn get(&self, id: usize) -> Option<&Arc<Mutex<UnixSocketInner<'a>>>> {
        self.inner.get(&id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Arc<Mutex<UnixSocketInner<'a>>>> {
        self.inner.get_mut(&id)
    }
}
static UNIX_TABLE: LazyInit<RwLock<HashMapWarpper>> = LazyInit::new();
/*lazy_static::lazy_static! {
    static ref UNIX_TABLE: RwLock<HashMapWarpper> = {
        let unix_table = HashMapWarpper::new();
        RwLock::new(unix_table)
    };
}*/

#[derive(Debug)]
pub enum UnixSocketType {
    SockStream,
    SockDgram,
    SockSeqpacket,
}

// State transitions:
// CLOSED -(connect)-> BUSY -> CONNECTING -> CONNECTED -(shutdown)-> BUSY -> CLOSED
//       |
//       |-(listen)-> BUSY -> LISTENING -(shutdown)-> BUSY -> CLOSED
//       |
//        -(bind)-> BUSY -> CLOSED
#[derive(Clone, Copy, Debug)]
pub enum UnixSocketStatus {
    Closed,
    Busy,
    Connecting,
    Connected,
    Listening,
}

impl UnixSocket {
    /// create a new socket
    /// only support sock_stream
    pub fn new(_type: UnixSocketType) -> Self {
        match _type {
            UnixSocketType::SockDgram | UnixSocketType::SockSeqpacket => unimplemented!(),
            UnixSocketType::SockStream => {
                let mut unixsocket = UnixSocket {
                    sockethandle: None,
                    unixsocket_type: _type,
                };
                let handle = UNIX_TABLE.write().add(Arc::new(Mutex::new(UnixSocketInner::new()))).unwrap();
                unixsocket.set_sockethandle(handle);
                unixsocket
            },
        }
    }

    pub fn set_sockethandle(&mut self, fd: usize) {
        self.sockethandle = Some(fd);
    }

    pub fn get_sockethandle(&self) -> usize {
        self.sockethandle.unwrap()
    }

    pub fn get_peerhandle(&self) -> Option<usize> {
        UNIX_TABLE.read().get(self.get_sockethandle()).unwrap().lock().get_peersocket()
    }

    pub fn get_state(&self) -> UnixSocketStatus {
        UNIX_TABLE.read().get(self.get_sockethandle()).unwrap().lock().status
    }

    pub fn enqueue_buf(&mut self, data: &[u8]) -> AxResult<usize> {
        match self.get_state() {
            UnixSocketStatus::Closed => Err(AxError::BadState),
            _ => Ok(UNIX_TABLE.write().get_mut(self.get_sockethandle()).unwrap().lock().buf.enqueue_slice(data))
        }
    }

    pub fn dequeue_buf(&mut self, data: &mut [u8]) -> AxResult<usize> {
        match self.get_state() {
            UnixSocketStatus::Closed => Err(AxError::BadState),
            _ => {
                if UNIX_TABLE.write().get_mut(self.get_sockethandle()).unwrap().lock().buf.is_empty() {
                    return Err(AxError::WouldBlock);
                }
                Ok(UNIX_TABLE.write().get_mut(self.get_sockethandle()).unwrap().lock().buf.dequeue_slice(data))
            }
        }
    }

    // TODO: bind to file system
    pub fn bind(&mut self, addr: SockaddrUn) -> LinuxResult {
        let now_state = self.get_state();
        match now_state {
            UnixSocketStatus::Closed => {
                {
                    let inode_addr = get_inode(addr)?;
                    UNIX_TABLE.write().replace_handle(self.get_sockethandle(), inode_addr);
                    self.set_sockethandle(inode_addr);
                }
                let mut binding = UNIX_TABLE.write();
                let mut socket_inner = binding.get_mut(self.get_sockethandle()).unwrap().lock();
                socket_inner.addr.lock().set_addr(&addr);
                socket_inner.set_state(UnixSocketStatus::Busy);
                Ok(())
            }
            _ => {
                Err(LinuxError::EINVAL)
            }
        }
        
    }

    pub fn send(&self, buf: &[u8]) -> LinuxResult<usize> {
        match self.unixsocket_type {
            UnixSocketType::SockDgram | UnixSocketType::SockSeqpacket => Err(LinuxError::ENOTCONN),
            UnixSocketType::SockStream => {
                loop {
                    let now_state = self.get_state();
                    match now_state {
                        UnixSocketStatus::Connecting => {
                            yield_now();
                        }
                        UnixSocketStatus::Connected => {
                            let peer_handle = UNIX_TABLE.read().get(self.get_sockethandle()).unwrap().lock().get_peersocket().unwrap();
                            return Ok(UNIX_TABLE.write().get_mut(peer_handle).unwrap().lock().buf.enqueue_slice(buf));
                        },
                        _ => { return Err(LinuxError::ENOTCONN); },
                    }
                } 
            }
        }
    }
    pub fn recv(&self, buf: &mut [u8], flags: i32) -> LinuxResult<usize> {
        match self.unixsocket_type {
            UnixSocketType::SockDgram | UnixSocketType::SockSeqpacket => Err(LinuxError::ENOTCONN),
            UnixSocketType::SockStream => {
                loop {
                    let now_state = self.get_state();
                    match now_state {
                        UnixSocketStatus::Connecting => {
                            yield_now();
                        }
                        UnixSocketStatus::Connected => {
                            {
                                if UNIX_TABLE.read().get(self.get_sockethandle()).unwrap().lock().buf.is_empty() {
                                    yield_now();
                                }
                            }
                            return Ok(UNIX_TABLE.read().get(self.get_sockethandle()).unwrap().lock().buf.dequeue_slice(buf));
                        },
                        _ => { return Err(LinuxError::ENOTCONN); },
                    }
                }
            }
        }
    }
    pub fn poll(&self) -> LinuxResult<PollState> {
        unimplemented!()
    }

    pub fn local_addr(&self) -> LinuxResult<SocketAddr> {
        unimplemented!()
    }

    fn fd(&self) -> c_int {
        UNIX_TABLE.write().get_mut(self.get_sockethandle()).unwrap().lock().addr.lock().sun_path[0] as _
    }

    pub fn peer_addr(&self) -> AxResult<SockaddrUn> {
        let now_state = self.get_state();
        match now_state {
            UnixSocketStatus::Connected | UnixSocketStatus::Listening => {
                let peer_sockethandle = self.get_peerhandle().unwrap();
                Ok(UNIX_TABLE.read().get(peer_sockethandle).unwrap().lock().get_addr())
            }
            _ => Err(AxError::NotConnected),
        }
    }

    // TODO: check file system
    pub fn connect(&mut self, addr: SockaddrUn) -> LinuxResult {
        //a new block is needed to free rwlock
        {
            let binding = UNIX_TABLE.write();
            let (remote_sockethandle, remote_socket) = binding.find(|socket| {
                socket.lock().addr.lock().sun_path == addr.sun_path
            }).unwrap();
            //let mut remote_socket = UNIX_TABLE.read().get_mut(remote_sockethandle).unwrap();
            /*let data = unsafe {
                let bytes = core::mem::transmute::<&usize, &[u8; core::mem::size_of::<usize>()]>(&self.get_sockethandle().into());
                &bytes[..]
            };*/
            let data = &self.get_sockethandle().to_ne_bytes();
            let res = remote_socket.lock().buf.enqueue_slice(data);
        }
        let mut binding = UNIX_TABLE.write();
        let mut socket_inner = binding.get_mut(self.get_sockethandle()).unwrap().lock();
        socket_inner.set_state(UnixSocketStatus::Connecting);
        Ok(())
    }

    pub fn sendto(&self, buf: &[u8], addr:SockaddrUn) -> LinuxResult<usize> {
        unimplemented!()
    }

    pub fn recvfrom(&self, buf: &mut [u8]) -> LinuxResult<(usize, Option<SocketAddr>)> {
        unimplemented!()
    }

    // TODO: check file system
    pub fn listen(&mut self) -> LinuxResult {
        let now_state = self.get_state();
        match now_state {
            UnixSocketStatus::Busy => {
                let mut binding = UNIX_TABLE.write();
                let mut socket_inner = binding.get_mut(self.get_sockethandle()).unwrap().lock();
                socket_inner.set_state(UnixSocketStatus::Listening);
                Ok(())
            }
            _ => {
                Ok(())//ignore simultaneous `listen`s.
            }
        }
    }

    pub fn accept(&mut self) -> AxResult<UnixSocket> {
        let now_state = self.get_state();
        match now_state {
            UnixSocketStatus::Listening => {
                //buf dequeue as handle to get socket
                loop {
                    let data: &mut [u8] = &mut [0u8; core::mem::size_of::<usize>()];
                    let res = self.dequeue_buf(data);
                    let test_state = self.get_state();
                    match res {
                        Ok(len) => {
                            let mut array = [0u8; core::mem::size_of::<usize>()];
                            array.copy_from_slice(data);
                            let remote_handle = usize::from_ne_bytes(array);
                            let unix_socket = UnixSocket::new(UnixSocketType::SockStream);
                            {
                                let mut binding = UNIX_TABLE.write();
                                let remote_socket =  binding.get_mut(remote_handle).unwrap();
                                remote_socket.lock().set_peersocket(unix_socket.get_sockethandle());
                                remote_socket.lock().set_state(UnixSocketStatus::Connected);
                            }
                            let mut binding = UNIX_TABLE.write();
                            let mut socket_inner = binding.get_mut(unix_socket.get_sockethandle()).unwrap().lock();
                            socket_inner.set_peersocket(remote_handle);
                            socket_inner.set_state(UnixSocketStatus::Connected);
                            return Ok(unix_socket);
                        },
                        Err(AxError::WouldBlock) => {
                            yield_now();
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
            },
            _ => ax_err!(InvalidInput, "socket accept() failed: not listen")
        }
    }

    pub fn shutdown(&self) -> LinuxResult {
        unimplemented!()
    }

    pub fn set_nonblocking(&self, nonblocking: bool) {
        unimplemented!()
    }
}

pub(crate) fn init_unix() {
    UNIX_TABLE.init_by(RwLock::new(HashMapWarpper::new()));
}

/*
let mut fd: i32 = -1;
        let mut now_file_fd: i32 = 3;
        while now_file_fd < RUX_FILE_LIMIT.try_into().unwrap() {
            match Socket::from_fd(now_file_fd) {
                Ok(socket) => {
                    if let Ok(socket) = Arc::try_unwrap(socket) {
                        match socket  {
                            Socket::Unix(unixsocket) => {
                                if unixsocket.lock().addr.lock().sun_path == addr.sun_path {
                                    fd = now_file_fd;
                                    unixsocket.listen.lock().push(self.get_socket_fd());
                                    break;
                                } else {
                                    now_file_fd += 1;
                                }
                            }
                            _ => {
                                now_file_fd += 1;
                            }
                        }
                    }
                }
                _ => {
                    now_file_fd += 1;
                }
            }
        }
        if fd == -1 {
            Err(LinuxError::ENOENT)
        }
        else {
            self.peer_fd
            .store(fd as _, core::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
*/