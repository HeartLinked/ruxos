use alloc::sync::Arc;
use axerrno::{LinuxError, LinuxResult};
use axfs_vfs::{impl_vfs_non_dir_default, VfsNodeAttr, VfsNodeOps, VfsResult};
use core::sync::atomic::{AtomicUsize, Ordering};
use log::debug;
use spin::Mutex;

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    Full,
    Empty,
    Normal,
}

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

    pub fn read(&self, buf: &mut [u8]) -> LinuxResult<usize> {
        debug!("read data from fifo");
        let mut read_size = 0usize;
        let max_len = buf.len();

        loop {
            let mut ring_buffer = self.buffer.lock();
            let available = ring_buffer.available_read();
            if available == 0 {
                if self.writers.load(Ordering::SeqCst) == 0 {
                    // only EOF when no writer and no data
                    return Ok(0);
                } else {
                    drop(ring_buffer);
                    sched_yield();
                    // must continue to wait for data
                    continue;
                }
            }
            for _ in 0..available {
                if read_size == max_len {
                    return Ok(read_size);
                }
                buf[read_size] = ring_buffer.read_byte();
                read_size += 1;
            }
            break;
        }
        Ok(read_size)
    }

    pub fn write(&self, buf: &[u8]) -> LinuxResult<usize> {
        debug!("write data to fifo");
        let mut write_size = 0usize;
        let max_len = buf.len();

        loop {
            let mut ring_buffer = self.buffer.lock();

            if self.readers.load(Ordering::SeqCst) == 0 {
                return Err(LinuxError::EPIPE);
            }

            let available = ring_buffer.available_write();
            if available == 0 {
                drop(ring_buffer);
                sched_yield();
                continue;
            }

            for _ in 0..available {
                if write_size == max_len {
                    break;
                }
                ring_buffer.write_byte(buf[write_size]);
                write_size += 1;
            }

            if write_size > 0 {
                return Ok(write_size);
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
}

impl VfsNodeOps for FifoNode {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new_fifo(self.ino, 0, 0))
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        Ok(self.fifo.read(buf).unwrap_or(0))
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        Ok(self.fifo.write(buf).unwrap_or(0))
    }

    fn fifo_has_readers(&self) -> bool {
        self.fifo.readers.load(Ordering::SeqCst) > 0
    }

    fn open_fifo(&self, read: bool, write: bool, non_blocking: bool) -> VfsResult {
        debug!("open a fifo node");
        if read {
            self.fifo.readers.fetch_add(1, Ordering::SeqCst);
            if !non_blocking {
                while self.fifo.writers.load(Ordering::SeqCst) == 0 {
                    sched_yield();
                }
            }
        }
        if write {
            self.fifo.writers.fetch_add(1, Ordering::SeqCst);
            if !non_blocking {
                while self.fifo.readers.load(Ordering::SeqCst) == 0 {
                    sched_yield();
                }
            }
        }
        Ok(())
    }

    fn release_fifo(&self, read: bool, write: bool) -> VfsResult {
        debug!("release a fifo node");
        if read {
            self.fifo.readers.fetch_sub(1, Ordering::SeqCst);
        }
        if write {
            self.fifo.writers.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        Ok(())
    }

    fn fsync(&self) -> VfsResult {
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
