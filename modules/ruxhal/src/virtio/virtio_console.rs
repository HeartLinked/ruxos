/* Copyright (c) [2023] [Syswonder Community]
 *   [Ruxos] is licensed under Mulan PSL v2.
 *   You can use this software according to the terms and conditions of the Mulan PSL v2.
 *   You may obtain a copy of Mulan PSL v2 at:
 *               http://license.coscl.org.cn/MulanPSL2
 *   THIS SOFTWARE IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OF ANY KIND, EITHER EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO NON-INFRINGEMENT, MERCHANTABILITY OR FIT FOR A PARTICULAR PURPOSE.
 *   See the Mulan PSL v2 for more details.
 */

//! virtio_console
use crate::mem::phys_to_virt;
use crate::virtio::virtio_hal::VirtIoHalImpl;
use driver_console::ConsoleDriverOps;
use driver_virtio::VirtIoConsoleDev;
use spinlock::SpinNoIrq;

#[cfg(feature = "irq")]
const BUFFER_SIZE: usize = 128;

#[cfg(feature = "irq")]
struct RxRingBuffer {
    buffer: [u8; BUFFER_SIZE],
    head: usize,
    tail: usize,
    empty: bool,
}

/// The UART RxRingBuffer
#[cfg(feature = "irq")]
impl RxRingBuffer {
    /// Create a new ring buffer
    const fn new() -> Self {
        RxRingBuffer {
            buffer: [0_u8; BUFFER_SIZE],
            head: 0_usize,
            tail: 0_usize,
            empty: true,
        }
    }

    /// Push a byte into the buffer
    fn push(&mut self, n: u8) {
        if self.tail != self.head || self.empty {
            self.buffer[self.tail] = n;
            self.tail = (self.tail + 1) % BUFFER_SIZE;
            self.empty = false;
        }
    }

    /// Pop a byte from the buffer
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

/// The UART driver
struct UartDrv {
    inner: Option<SpinNoIrq<VirtIoConsoleDev<VirtIoHalImpl, VirtIoTransport>>>,
    buffer: [u8; 20000],
    #[cfg(feature = "irq")]
    irq_buffer: SpinNoIrq<RxRingBuffer>,
    pointer: usize,
    addr: usize,
}

/// The UART driver instance
static mut UART: UartDrv = UartDrv {
    inner: None,
    buffer: [0; 20000],
    #[cfg(feature = "irq")]
    irq_buffer: SpinNoIrq::new(RxRingBuffer::new()),
    pointer: 0,
    addr: 0,
};

/// Writes a byte to the console.
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
            uart.putchar(c);
        } else {
            UART.buffer[UART.pointer] = c;
            UART.pointer += 1;
        }
    }
}

/// Reads a byte from the console.
pub fn getchar() -> Option<u8> {
    unsafe {
        #[cfg(feature = "irq")]
        return UART.irq_buffer.lock().pop();
        #[cfg(not(feature = "irq"))]
        if let Some(ref mut uart_inner) = UART.inner {
            let mut uart = uart_inner.lock();
            return uart.getchar();
        } else {
            None
        }
    }
}

/// probe virtio console directly
pub fn directional_probing() {
    info!("Initiating VirtIO Console ...");
    let uart_base: usize = ruxconfig::VIRTIO_CONSOLE_PADDR;
    let uart_reg: usize = 0x200;
    if let Some(dev) = probe_mmio(uart_base, uart_reg) {
        unsafe {
            UART.inner = Some(SpinNoIrq::new(dev));
            UART.addr = uart_base;
        }
    }
    info!("Output now redirected to VirtIO Console!");
}

/// enable virtio console interrupt
pub fn enable_interrupt() {
    #[cfg(all(feature = "irq", target_arch = "aarch64"))]
    {
        let virtio_console_irq_num = ruxconfig::VIRTIO_CONSOLE_IRQ + 32;
        info!("Initiating VirtIO Console interrupt ...");
        info!("IRQ ID: {}", virtio_console_irq_num);
        crate::irq::register_handler(virtio_console_irq_num, irq_handler);
        crate::irq::set_enable(virtio_console_irq_num, true);
        ack_interrupt();
        info!("Interrupt enabled!");
    }
}

/// virtio console interrupt handler
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

/// Acknowledge the interrupt
pub fn ack_interrupt() {
    #[cfg(feature = "irq")]
    unsafe {
        if let Some(ref mut uart_inner) = UART.inner {
            let mut uart = uart_inner.lock();
            uart.ack_interrupt()
                .expect("Virtio_console ack interrupt error");
        }
    }
}

/// Check if the address is the probe address
pub fn is_probe(addr: usize) -> bool {
    unsafe { addr == UART.addr }
}

/// Probe the virtio console
fn probe_mmio(
    mmio_base: usize,
    mmio_size: usize,
) -> Option<VirtIoConsoleDev<VirtIoHalImpl, VirtIoTransport>> {
    let base_vaddr = phys_to_virt(mmio_base.into());
    if let Some((ty, transport)) =
        driver_virtio::probe_mmio_device(base_vaddr.as_mut_ptr(), mmio_size)
    {
        if ty == driver_common::DeviceType::Char {
            info!(
                "VirtIO Console found at {:#x} size {:#x}",
                mmio_base, mmio_size
            );
            return match VirtIoConsoleDev::try_new(transport) {
                Ok(dev) => Some(dev),
                Err(_e) => None,
            };
        }
    }
    None
}

/// Virtio transport type
type VirtIoTransport = driver_virtio::MmioTransport;
