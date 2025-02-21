/* Copyright (c) [2023] [Syswonder Community]
 *   [Ruxos] is licensed under Mulan PSL v2.
 *   You can use this software according to the terms and conditions of the Mulan PSL v2.
 *   You may obtain a copy of Mulan PSL v2 at:
 *               http://license.coscl.org.cn/MulanPSL2
 *   THIS SOFTWARE IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OF ANY KIND, EITHER EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO NON-INFRINGEMENT, MERCHANTABILITY OR FIT FOR A PARTICULAR PURPOSE.
 *   See the Mulan PSL v2 for more details.
 */
use ruxtask::sync::{AtomicUsize, Mutex, WaitQueue};

pub struct Fifo {
    buffer: Mutex<PipeRingBuffer>,      
    readers: AtomicUsize,               
    writers: AtomicUsize,               
    read_wait_queue: Mutex<WaitQueue>,  // 读端等待队列
    write_wait_queue: Mutex<WaitQueue>, // 写端等待队列
}

pub struct FifoEndpoint {
    fifo: Arc<Fifo>,      
    is_reader: bool,      
    non_blocking: bool,   
}

impl Fifo {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(PipeRingBuffer::new()),
            readers: AtomicUsize::new(0),
            writers: AtomicUsize::new(0),
            read_wait_queue: Mutex::new(WaitQueue::new()),
            write_wait_queue: Mutex::new(WaitQueue::new()),
        })
    }

    pub fn open_read(&self, non_blocking: bool) -> Result<FifoEndpoint> {
        let endpoint = FifoEndpoint {
            fifo: self.clone(),
            is_reader: true,
            non_blocking,
        };

        if non_blocking && self.writers.load(Ordering::Relaxed) == 0 {
            return Err(LinuxError::ENXIO);
        }

        while self.writers.load(Ordering::Relaxed) == 0 {
            if non_blocking {
                return Err(LinuxError::EAGAIN);
            }
            self.read_wait_queue.lock().sleep(); 
        }

        self.readers.fetch_add(1, Ordering::Relaxed);
        Ok(endpoint)
    }

    pub fn open_write(&self, non_blocking: bool) -> Result<FifoEndpoint> {
        let endpoint = FifoEndpoint {
            fifo: self.clone(),
            is_reader: false,
            non_blocking,
        };


        if non_blocking && self.readers.load(Ordering::Relaxed) == 0 {
            return Err(LinuxError::ENXIO);
        }

        while self.readers.load(Ordering::Relaxed) == 0 {
            if non_blocking {
                return Err(LinuxError::EAGAIN);
            }
            self.write_wait_queue.lock().sleep();
        }

        self.writers.fetch_add(1, Ordering::Relaxed);
        Ok(endpoint)
    }
}

impl FileLike for FifoEndpoint {
    fn read(&self, buf: &mut [u8]) -> LinuxResult<usize> {
        if !self.is_reader {
            return Err(LinuxError::EPERM);
        }

        let mut buffer = self.fifo.buffer.lock();
        loop {
            let available = buffer.available_read();
            if available > 0 {
                let read_size = 0;
                for i in 0..available {
                    if read_size >= buf.len() {
                        break;
                    }
                    buf[read_size] = buffer.read_byte();
                    read_size += 1;
                }
                return Ok(read_size);
            } else {

                if self.fifo.writers.load(Ordering::Relaxed) == 0 {
                    return Ok(0); 
                }
                if self.non_blocking {
                    return Err(LinuxError::EAGAIN);
                }

                drop(buffer);
                self.fifo.read_wait_queue.lock().sleep();
                buffer = self.fifo.buffer.lock();
            }
        }
    }

    fn write(&self, buf: &[u8]) -> LinuxResult<usize> {
        if self.is_reader {
            return Err(LinuxError::EPERM);
        }

        let mut buffer = self.fifo.buffer.lock();
        let mut written = 0;
        while written < buf.len() {
            let available = buffer.available_write();
            if available == 0 {

                if self.fifo.readers.load(Ordering::Relaxed) == 0 {
                    return Err(LinuxError::EPIPE); 
                }
                if self.non_blocking {
                    return Ok(written);
                }
                drop(buffer);
                self.fifo.write_wait_queue.lock().sleep();
                buffer = self.fifo.buffer.lock();
                continue;
            }
            for i in 0..available {
                if written >= buf.len() {
                    break;
                }
                buffer.write_byte(buf[written]);
                written += 1;
            }
        }
        Ok(written)
    }
}