use alloc::sync::Arc;
use axerrno::{LinuxError, LinuxResult};
use axfs_vfs::{impl_vfs_non_dir_default, VfsNodeAttr, VfsNodeOps, VfsResult};
use core::ffi::c_int;
use core::sync::atomic::{AtomicUsize, Ordering};
use log::warn;
use spin::Mutex;
// use ruxos_posix_api::ctypes;

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    Full,
    Empty,
    Normal,
}

// const RING_BUFFER_SIZE: usize = ruxconfig::PIPE_BUFFER_SIZE;
const RING_BUFFER_SIZE: usize = 1024;

pub struct PipeRingBuffer {
    arr: [u8; RING_BUFFER_SIZE],
    head: usize,
    tail: usize,
    status: RingBufferStatus,
}

impl PipeRingBuffer {
    pub const fn new() -> Self {
        Self {
            arr: [0; RING_BUFFER_SIZE],
            head: 0,
            tail: 0,
            status: RingBufferStatus::Empty,
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.status = RingBufferStatus::Normal;
        self.arr[self.tail] = byte;
        self.tail = (self.tail + 1) % RING_BUFFER_SIZE;
        if self.tail == self.head {
            self.status = RingBufferStatus::Full;
        }
    }

    pub fn read_byte(&mut self) -> u8 {
        self.status = RingBufferStatus::Normal;
        let c = self.arr[self.head];
        self.head = (self.head + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::Empty;
        }
        c
    }

    pub fn available_read(&self) -> usize {
        if self.status == RingBufferStatus::Empty {
            0
        } else if self.tail > self.head {
            self.tail - self.head
        } else {
            self.tail + RING_BUFFER_SIZE - self.head
        }
    }

    pub fn available_write(&self) -> usize {
        if self.status == RingBufferStatus::Full {
            0
        } else {
            RING_BUFFER_SIZE - self.available_read()
        }
    }
}

pub struct Fifo {
    buffer: Arc<Mutex<PipeRingBuffer>>,
    readers: AtomicUsize,
    writers: AtomicUsize,
}

impl Fifo {
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(PipeRingBuffer::new())),
            readers: AtomicUsize::new(0),
            writers: AtomicUsize::new(0),
        }
    }

    pub fn register_reader(&self) {
        self.readers.fetch_add(1, Ordering::SeqCst);
    }

    pub fn unregister_reader(&self) {
        self.readers.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn register_writer(&self) {
        self.writers.fetch_add(1, Ordering::SeqCst);
    }

    pub fn unregister_writer(&self) {
        self.writers.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn read(&self, buf: &mut [u8]) -> LinuxResult<usize> {
        let mut read_size = 0usize;
        let max_len = buf.len();
        let mut ring_buffer = self.buffer.lock();
        loop {
            let available = ring_buffer.available_read();
            if available == 0 {
                if self.writers.load(Ordering::SeqCst) == 0 {
                    return Ok(0);
                } else {
                    drop(ring_buffer);
                    sched_yield();
                    ring_buffer = self.buffer.lock();
                }
            } else {
                break;
            }
        }
        let available = ring_buffer.available_read();
        for _ in 0..available {
            if read_size == max_len {
                return Ok(read_size);
            }
            buf[read_size] = ring_buffer.read_byte();
            read_size += 1;
        }
        Ok(read_size)
    }

    pub fn write(&self, buf: &[u8]) -> LinuxResult<usize> {
        let mut write_size = 0usize;
        let max_len = buf.len();
        loop {
            let mut ring_buffer = self.buffer.lock();
            let available = ring_buffer.available_write();
            if available == 0 {
                drop(ring_buffer);
                if self.readers.load(Ordering::SeqCst) == 0 {
                    return Err(LinuxError::EPIPE);
                }
                sched_yield();
                continue;
            }
            for _ in 0..available {
                if write_size == max_len {
                    return Ok(write_size);
                }
                ring_buffer.write_byte(buf[write_size]);
                write_size += 1;
            }
        }
    }
}

pub struct FifoNode {
    ino: u64,
    fifo: Fifo,
}

impl FifoNode {
    pub fn new(ino: u64) -> Self {
        Self {
            ino,
            fifo: Fifo::new(),
        }
    }

    pub fn open(&self, flags: c_int) {
        // 假设 O_RDONLY、O_WRONLY、O_RDWR 标志分别表示只读、只写和读写
        // if flags & libc::O_ACCMODE == libc::O_RDONLY {
        //     self.fifo.register_reader();
        // } else if flags & libc::O_ACCMODE == libc::O_WRONLY {
        //     self.fifo.register_writer();
        // } else if flags & libc::O_ACCMODE == libc::O_RDWR {

        // TODO: fix this
        self.fifo.register_reader();
        self.fifo.register_writer();
        // }
    }
}

impl VfsNodeOps for FifoNode {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new_fifo(self.ino, 0, 0))
    }

    // FIFO 是一种流式设备，因此 offset 无意义，直接调用 FIFO 的 read 实现
    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // 此处直接调用 FIFO 的 read 方法
        Ok(self.fifo.read(buf).unwrap_or(0))
    }

    // 同理，write 操作忽略 offset
    fn write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        Ok(self.fifo.write(buf).unwrap_or(0))
    }

    // FIFO 不支持截断操作，可以直接忽略
    fn truncate(&self, _size: u64) -> VfsResult {
        Ok(())
    }

    impl_vfs_non_dir_default! {}
}

fn sched_yield() {
    #[cfg(feature = "multitask")]
    ruxtask::yield_now();
    #[cfg(not(feature = "multitask"))]
    if cfg!(feature = "irq") {
        ruxhal::arch::wait_for_irqs();
    } else {
        core::hint::spin_loop();
    }
}
