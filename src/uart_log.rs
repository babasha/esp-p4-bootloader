//! Internal UART0 raw logger used by the boot-up modules (`bod`, `wdt`,
//! `mpll`) before the application's logging stack is up.
//!
//! Bytes go straight to the UART0 TX FIFO at `0x500C_A000`. Every write
//! checks `txfifo_count()` against the 128-byte hardware capacity first
//! and spins on backpressure — without that, ~12 lines of BEFORE/AFTER
//! tracing overrun the FIFO (~11 ms at 115200 baud) and bytes are
//! silently dropped, which makes a healthy boot look truncated.
//!
//! Single-hart at boot, no IRQs, so no synchronisation needed.

#![allow(unsafe_code)]

use core::ptr;

const UART0_FIFO: *mut u32 = 0x500C_A000 as *mut u32;
const UART0_STATUS: *const u32 = 0x500C_A01C as *const u32;
const TXFIFO_CNT_SHIFT: u32 = 16;
const TXFIFO_CNT_MASK: u32 = 0xFF << TXFIFO_CNT_SHIFT;
const TXFIFO_CAPACITY: u32 = 128;

#[inline(always)]
fn txfifo_count() -> u32 {
    (unsafe { ptr::read_volatile(UART0_STATUS) } & TXFIFO_CNT_MASK) >> TXFIFO_CNT_SHIFT
}

#[inline(always)]
pub(crate) fn uart_byte(b: u8) {
    while txfifo_count() >= TXFIFO_CAPACITY {
        core::hint::spin_loop();
    }
    unsafe { ptr::write_volatile(UART0_FIFO, b as u32) };
}

pub(crate) fn uart_str(s: &str) {
    for &b in s.as_bytes() {
        uart_byte(b);
    }
}

pub(crate) fn uart_hex32(prefix: &str, v: u32) {
    uart_str(prefix);
    let hex = b"0123456789ABCDEF";
    for i in 0..8 {
        uart_byte(hex[((v >> ((7 - i) * 4)) & 0xF) as usize]);
    }
    uart_str("\r\n");
}
