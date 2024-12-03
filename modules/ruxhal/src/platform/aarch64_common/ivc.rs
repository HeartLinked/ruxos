/* Copyright (c) [2024] [Syswonder Community]
 *   [Ruxos] is licensed under Mulan PSL v2.
 *   You can use this software according to the terms and conditions of the Mulan PSL v2.
 *   You may obtain a copy of Mulan PSL v2 at:
 *               http://license.coscl.org.cn/MulanPSL2
 *   THIS SOFTWARE IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OF ANY KIND, EITHER EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO NON-INFRINGEMENT, MERCHANTABILITY OR FIT FOR A PARTICULAR PURPOSE.
 *   See the Mulan PSL v2 for more details.
 */

use core::alloc::Layout;
use core::ptr;
use core::ptr::NonNull;
use axalloc:: {global_allocator, GlobalAllocator};

// pub const CONFIG_MAX_IVC_CONFIGS: usize = 0x2;
// pub const HVISOR_HC_IVC_INFO: usize = 0x5;
// pub const HVISOR_HC_IVC_INFO_ALIGN: usize = 0x8;
// pub const HVISOR_HC_IVC_INFO_SIZE: usize = 56;
// pub const __PA: usize = 0xffff_0000_0000_0000;

// #[repr(C)]
// #[derive(Debug)]
// struct IvCInfo {
//     len: u64,   // The number of IVC shared memory 
//     ivc_ct_ipas: [u64; CONFIG_MAX_IVC_CONFIGS], // Control Table IPA
//     ivc_shmem_ipas: [u64; CONFIG_MAX_IVC_CONFIGS], // Share memory IPA
//     ivc_ids: [u32; CONFIG_MAX_IVC_CONFIGS], // IVC id; the ivc id of zones that communicate with each other have to be the same
//     ivc_irqs: [u32; CONFIG_MAX_IVC_CONFIGS], // irq number
// }

// #[repr(C)]
// #[derive(Debug)]
// struct ControlTable {
//     ivc_id: u32,
//     max_peers: u32,
//     rw_sec_size: u32,
//     out_sec_size: u32,
//     peer_id: u32,
//     ipi_invoke: u32,
// }

// pub fn write_to_address(addr: u64, data: &str) -> Result<(), &'static str> {
//     let len = data.len();

//     if addr == 0 {
//         return Err("Invalid address: 0x0");
//     }

//     unsafe {
//         let ptr = addr as *mut u8;

//         for (i, &byte) in data.as_bytes().iter().enumerate() {
//             let target_ptr = ptr.add(i); 
//             *target_ptr = byte; 
//         }

//         let null_ptr = ptr.add(len);
//         *null_ptr = 0; 
//     }

//     Ok(())
// }

// pub fn ivc() {

//     let allocator = global_allocator();
//     let alloc_size = HVISOR_HC_IVC_INFO_SIZE;
//     let align = HVISOR_HC_IVC_INFO_ALIGN;

//     let layout = Layout::from_size_align(alloc_size, align).unwrap();
//     // ptr is a NonNull<u8> pointer
//     let ptr = allocator.alloc(layout).expect("Memory allocate FAILED!!"); 

//     // the virtual address of the IVC Info
//     let vpa_ivcinfo = ptr.as_ptr() as usize;
//     // the physical address of the IVC Info
//     let pa_ivcinfo: usize = vpa_ivcinfo - __PA;

//     info!("The memory address of the IVC Info: VA:0x{:x}, IPA:0x{:x}", vpa_ivcinfo, pa_ivcinfo);

//     ivc_hvc_call(HVISOR_HC_IVC_INFO as u32, pa_ivcinfo, HVISOR_HC_IVC_INFO_SIZE);
//     info!("ivc_hvc_call finished.");
    
//     let _vpa_ivcinfo = vpa_ivcinfo as *const IvCInfo;

//     // 先解引用指针
//     let ivc_info: &IvCInfo = unsafe { &*_vpa_ivcinfo };

//     // info!("len: 0x{:x}", ivc_info.len);
//     // info!("ivc_ct_ipas[0]: 0x{:x}", ivc_info.ivc_ct_ipas[0]);
//     // info!("ivc_shmem_ipas[0]: 0x{:x}", ivc_info.ivc_shmem_ipas[0]);
//     // info!("ivc_ids[0]: 0x{:x}", ivc_info.ivc_ids[0]);
//     // info!("ivc_irqs[0]: {}", ivc_info.ivc_irqs[0]);

//     // 获取控制表指针，并进行类型转换
//     let control_table_ptr = (ivc_info.ivc_ct_ipas[0] + __PA as u64) as *mut ControlTable;

//     // 通过 unsafe 进行可变引用转换
//     let control_table: &mut ControlTable = unsafe { &mut *control_table_ptr };

//     // info!("ivc_id: {}", control_table.ivc_id);
//     // info!("max_peers: {}", control_table.max_peers);
//     // info!("rw_sec_size: 0x{:x}", control_table.rw_sec_size);
//     // info!("out_sec_size: 0x{:x}", control_table.out_sec_size);
//     // info!("peer_id: {}", control_table.peer_id);

//     // 假设目标地址是一个 u64 类型的地址
//     let address: u64 = ivc_info.ivc_shmem_ipas[0] + 0x1000 + __PA as u64;
//     write_to_address(address, "Hello, World!").expect("Write to address failed.");
//     write_to_address(address, "This is a log message.").expect("Write to address failed.");

//     info!("write 'Hello, World!' to the shared memory.");

//     // 修改控制表的值
//     control_table.ipi_invoke = 0x0;

//     // 释放内存
//     allocator.dealloc(ptr, layout);
// }

// fn ivc_hvc_call(func: u32, arg0: usize, arg1: usize) -> usize {
//     let ret;
//     unsafe {
//         core::arch::asm!(
//             "hvc #4856",
//             inlateout("x0") func as usize => ret,
//             in("x1") arg0,
//             in("x2") arg1,
//             options(nostack) 
//         );
//         info!("ivc call: func: {:x}, arg0: 0x{:x}, arg1: 0x{:x}, ret: 0x{:x}", func, arg0, arg1, ret);
//     }
//     ret
// }

pub const CONFIG_MAX_IVC_CONFIGS: usize = 0x2;
pub const HVISOR_HC_IVC_INFO: usize = 0x5;
pub const HVISOR_HC_IVC_INFO_ALIGN: usize = 0x8;
pub const HVISOR_HC_IVC_INFO_SIZE: usize = 56;
pub const __PA: usize = 0xffff_0000_0000_0000;

#[repr(C)]
#[derive(Debug)]
struct IvCInfo {
    len: u64,   // The number of IVC shared memory 
    ivc_ct_ipas: [u64; CONFIG_MAX_IVC_CONFIGS], // Control Table IPA
    ivc_shmem_ipas: [u64; CONFIG_MAX_IVC_CONFIGS], // Share memory IPA
    ivc_ids: [u32; CONFIG_MAX_IVC_CONFIGS], // IVC id; the ivc id of zones that communicate with each other have to be the same
    ivc_irqs: [u32; CONFIG_MAX_IVC_CONFIGS], // irq number
}

#[repr(C)]
#[derive(Debug)]
struct ControlTable {
    ivc_id: u32,
    max_peers: u32,
    rw_sec_size: u32,
    out_sec_size: u32,
    peer_id: u32,
    ipi_invoke: u32,
}

fn write_to_address(addr: u64, data: &str) -> Result<(), &'static str> {
    let len = data.len();

    if addr == 0 {
        return Err("Invalid address: 0x0");
    }

    unsafe {
        let ptr = addr as *mut u8;

        for (i, &byte) in data.as_bytes().iter().enumerate() {
            let target_ptr = ptr.add(i); 
            *target_ptr = byte; 
        }

        let null_ptr = ptr.add(len);
        *null_ptr = 0; 
    }

    Ok(())
}

pub struct Connection<'a> {
    // Reference to the allocator
    allocator: &'a GlobalAllocator, 
    ivc_info_ptr: *mut IvCInfo,
    control_table_ptr: *mut ControlTable,
}

impl<'a> Connection<'a> {
    pub fn new() -> Self {
        // Get the reference to the global allocator
        let allocator = global_allocator(); 
        debug!("Connection created.");
        Connection {
            allocator,
            ivc_info_ptr: ptr::null_mut(),
            control_table_ptr: ptr::null_mut(),
        }
    }

    pub fn connect(&mut self) -> Result<(), &'static str> {
        let alloc_size = HVISOR_HC_IVC_INFO_SIZE;
        let align = HVISOR_HC_IVC_INFO_ALIGN;
        let layout = Layout::from_size_align(alloc_size, align).unwrap();
        
        let ptr = self.allocator.alloc(layout).expect("Memory allocation failed!");
        self.ivc_info_ptr = ptr.as_ptr() as *mut IvCInfo;

        let vpa_ivcinfo = self.ivc_info_ptr as usize;
        let pa_ivcinfo: usize = vpa_ivcinfo - __PA;

        debug!("The memory address of the IVC Info: VA: 0x{:x}, IPA: 0x{:x}", vpa_ivcinfo, pa_ivcinfo);

        ivc_hvc_call(HVISOR_HC_IVC_INFO as u32, pa_ivcinfo, HVISOR_HC_IVC_INFO_SIZE);
        debug!("ivc_hvc_call finished.");

        // Safety: At this point we know ivc_info_ptr is valid and allocated
        let ivc_info: &IvCInfo = unsafe { &*self.ivc_info_ptr };

        let control_table_ptr = (ivc_info.ivc_ct_ipas[0] + __PA as u64) as *mut ControlTable;
        self.control_table_ptr = control_table_ptr;

        info!("IVC Connection established.");
        Ok(())
    }

    pub fn send_message(&self, message: &str) -> Result<(), &'static str> {
        if self.ivc_info_ptr.is_null() {
            return Err("Not connected");
        }

        let ivc_info: &IvCInfo = unsafe { &*self.ivc_info_ptr };

        // target address is a u64 type address
        let address: u64 = ivc_info.ivc_shmem_ipas[0] + 0x1000 + __PA as u64;

        write_to_address(address, message)?;
        info!("Message written to shared memory: {}", message);

        let control_table: &mut ControlTable = unsafe { &mut *self.control_table_ptr };
        debug!("Ipi_invoke reset to inform Zone0 linux.");
        control_table.ipi_invoke = 0x0;

        Ok(())
    }

    pub fn close(&mut self) -> Result<(), &'static str> {
        if self.ivc_info_ptr.is_null() {
            return Err("Not connected");
        }
        // free the memory
        let layout = Layout::from_size_align(HVISOR_HC_IVC_INFO_SIZE, HVISOR_HC_IVC_INFO_ALIGN).unwrap();
        self.allocator.dealloc(unsafe { NonNull::new_unchecked(self.ivc_info_ptr as *mut u8) }, layout);
        info!("IVC Connection closed.");
        Ok(())
    }
}

fn ivc_hvc_call(func: u32, arg0: usize, arg1: usize) -> usize {
    let ret;
    unsafe {
        core::arch::asm!(
            "hvc #4856",
            inlateout("x0") func as usize => ret,
            in("x1") arg0,
            in("x2") arg1,
            options(nostack)
        );
        info!("Ivc call: func: {:x}, arg0: 0x{:x}, arg1: 0x{:x}, ret: 0x{:x}", func, arg0, arg1, ret);
    }
    ret
}

pub fn ivc_example() {
    let mut conn = Connection::new();

    // 建立连接
    if let Err(e) = conn.connect() {
        info!("Error connecting: {}", e);
        return;
    }

    // 发送信息
    if let Err(e) = conn.send_message("This is a log message.") {
        info!("Error sending message: {}", e);
    }

    // 关闭连接
    if let Err(e) = conn.close() {
        info!("Error closing connection: {}", e);
    }
}
