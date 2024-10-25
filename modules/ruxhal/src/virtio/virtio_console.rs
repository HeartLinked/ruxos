/* Copyright (c) [2023] [Syswonder Community]
 *   [Ruxos] is licensed under Mulan PSL v2.
 *   You can use this software according to the terms and conditions of the Mulan PSL v2.
 *   You may obtain a copy of Mulan PSL v2 at:
 *               http://license.coscl.org.cn/MulanPSL2
 *   THIS SOFTWARE IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OF ANY KIND, EITHER EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO NON-INFRINGEMENT, MERCHANTABILITY OR FIT FOR A PARTICULAR PURPOSE.
 *   See the Mulan PSL v2 for more details.
 */

//! virtio_console
use driver_virtio::VirtIoConsoleDev;
use spinlock::SpinNoIrq;
use driver_console::ConsoleDriverOps;
use crate::mem::phys_to_virt;
use crate::virtio::virtio_hal::VirtIoHalImpl;

#[cfg(feature = "irq")]
const BUFFER_SIZE: usize = 128;

#[cfg(feature = "irq")]
struct RxRingBuffer {
    buffer: [u8; BUFFER_SIZE],
    head: usize,
    tail: usize,
    empty: bool,
}

#[cfg(feature = "irq")]
impl RxRingBuffer {
    const fn new() -> Self {
        RxRingBuffer {
            buffer: [0_u8; BUFFER_SIZE],
            head: 0_usize,
            tail: 0_usize,
            empty: true,
        }
    }

    fn push(&mut self, n: u8) {
        if self.tail != self.head || self.empty {
            self.buffer[self.tail] = n;
            self.tail = (self.tail + 1) % BUFFER_SIZE;
            self.empty = false;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.empty {
            None
        } else {
            let ret = self.buffer[self.head];
            self.head = (self.head + 1) % BUFFER_SIZE;
            if self.head == self.tail {
                self.empty = true;
            }
            Some(ret)
        }
    }
}

struct UartDrv {
    inner: Option<SpinNoIrq<VirtIoConsoleDev<VirtIoHalImpl,VirtIoTransport>>>,
    buffer: [u8; 20000],
    #[cfg(feature = "irq")]
    irq_buffer: SpinNoIrq<RxRingBuffer>,
    pointer: usize,
    addr: usize,
}

static mut UART : UartDrv = UartDrv {
    inner: None,
    buffer: [0; 20000],
    #[cfg(feature = "irq")]
    irq_buffer: SpinNoIrq::new(RxRingBuffer::new()),
    pointer: 0,
    addr: 0,
};

pub fn putchar(c: u8) {
    unsafe {
        if let Some(ref mut uart_inner) = UART.inner {
            if UART.pointer > 0 {
                for i in 0..UART.pointer {
                    let mut uart = uart_inner.lock();
                    match UART.buffer[i] {
                        b'\n' => {
                            uart.putchar(b'\r');
                            uart.putchar(b'\n');
                        }
                        c => uart.putchar(c),
                    }
                }
                UART.pointer = 0;
                warn!("######################### The above content is printed from buffer! #########################");
            }
            let mut uart = uart_inner.lock();
            match c {
                b'\n' => {
                    uart.putchar(b'\r');
                    uart.putchar(b'\n');
                }
                c => uart.putchar(c),
            }
        } else {
            UART.buffer[UART.pointer] = c;
            UART.pointer += 1;
        }
    }
}


pub fn getchar() -> Option<u8> {
    unsafe {
        #[cfg(feature = "irq")]
        return UART.irq_buffer.lock().pop();
        #[cfg(not(feature = "irq"))]
        if let Some(ref mut uart_inner) = UART.inner {
            let mut uart = uart_inner.lock();
            return uart.getchar()
        } else {
            None
        }
    }
}


pub fn directional_probing() {
    info!("Initiating VirtIO Console ...");
    for reg in ruxconfig::VIRTIO_MMIO_REGIONS {
        {
            if let Some(dev) = probe_mmio(reg.0, reg.1) {
                unsafe {
                    UART.inner = Some(SpinNoIrq::new(dev));
                    UART.addr = reg.0;
                }
            }
        }
    }
    info!("Output now redirected to VirtIO Console!");
}

pub fn enable_interrupt() {
    #[cfg(feature = "irq")] {
        info!("Initiating VirtIO Console interrupt ...");
        info!("IRQ ID: {}", crate::platform::irq::UART_IRQ_NUM);
        crate::irq::register_handler(crate::platform::irq::UART_IRQ_NUM, irq_handler);
        crate::irq::set_enable(crate::platform::irq::UART_IRQ_NUM, true);
        ack_interrupt();
        info!("Interrupt enabled!");
    }
}


pub fn irq_handler() {
    #[cfg(feature = "irq")]
    unsafe {
        if let Some(ref mut uart_inner) = UART.inner {
            let mut uart = uart_inner.lock();
            if uart.ack_interrupt().unwrap() {
                while let Some(c) = uart.getchar() {
                    UART.irq_buffer.lock().push(c);
                }
            }
        }
    }
}


pub fn ack_interrupt() {
    #[cfg(feature = "irq")]
    unsafe {
        if let Some(ref mut uart_inner) = UART.inner {
            let mut uart = uart_inner.lock();
            uart.ack_interrupt().expect("Virtio_console ack interrupt error");
        }
    }
}

pub fn is_probe(addr: usize) -> bool {
    unsafe { return addr == UART.addr; }
}


fn probe_mmio(mmio_base: usize, mmio_size: usize) -> Option<VirtIoConsoleDev<VirtIoHalImpl,VirtIoTransport>> {
    let base_vaddr = phys_to_virt(mmio_base.into());
    if let Some((ty, transport)) =
        driver_virtio::probe_mmio_device(base_vaddr.as_mut_ptr(), mmio_size)
    {
        if ty == driver_common::DeviceType::Char {
            info!("VirtIO Console found at {:#x} size {:#x}", mmio_base, mmio_size);
            return match VirtIoConsoleDev::try_new(transport) {
                Ok(dev) => {
                    Some(dev)
                },
                Err(_e) => {
                    None
                }
            }
        }
    }
    None
}

type VirtIoTransport = driver_virtio::MmioTransport;

