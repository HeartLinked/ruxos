use core::ptr::NonNull;
use virtio_drivers::{BufferDirection, PhysAddr};
use axalloc::global_allocator;
use driver_virtio::VirtIoHal;
use crate::mem::{direct_virt_to_phys, phys_to_virt, virt_to_phys};

pub struct VirtIoHalImpl;

unsafe impl VirtIoHal for VirtIoHalImpl {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let vaddr = if let Ok(vaddr) = global_allocator().alloc_pages(pages, 0x1000) {
            vaddr
        } else {
            return (0, NonNull::dangling());
        };
        let paddr = direct_virt_to_phys(vaddr.into());
        let ptr = NonNull::new(vaddr as _).unwrap();
        (paddr.as_usize(), ptr)
    }

    unsafe fn dma_dealloc(_paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        global_allocator().dealloc_pages(vaddr.as_ptr() as usize, pages);
        0
    }

    #[inline]
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(phys_to_virt(paddr.into()).as_mut_ptr()).unwrap()
    }

    #[inline]
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        virt_to_phys(vaddr.into()).into()
    }

    #[inline]
    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}