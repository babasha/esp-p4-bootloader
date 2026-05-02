//! Disable hardware watchdogs left running by whatever firmware ran before us
//! (typically IDF in flash, when the chip warm-resets via `espflash --no-stub
//! --ram` and we land at our entry without a clean POR).
//!
//! On ESP32-P4 there are three watchdogs we care about at boot:
//! - LP_WDT (RTC watchdog) — `lp_wdt::config0.wdt_en`
//! - TIMG0 main WDT — `timg0::wdtconfig0.wdt_en`
//! - TIMG1 main WDT — `timg1::wdtconfig0.wdt_en`
//!
//! Each is gated by a write-protect register that requires writing the IDF
//! magic key `0x50D83AA1` before the disable can take effect, then writing 0
//! after to lock again.
//!
//! Without this, the IDF interrupt watchdog (TIMG1, ~300 ms default) fires
//! during PSRAM POR-settle waits and resets the chip back to ROM.

#[cfg(target_arch = "riscv32")]
use esp32p4 as pac;

/// IDF `RTC_CNTL_WDT_WKEY` / `TIMG_WDT_WKEY` value. Same magic for all three
/// watchdogs on ESP32-P4 (see `soc/lp_wdt_reg.h`, `soc/timer_group_reg.h`).
const WDT_KEY: u32 = 0x50D83AA1;

#[cfg(target_arch = "riscv32")]
fn uart_str(s: &str) {
    const UART0_FIFO: *mut u32 = 0x500C_A000 as *mut u32;
    for &b in s.as_bytes() {
        unsafe { core::ptr::write_volatile(UART0_FIFO, b as u32) };
    }
}

#[cfg(target_arch = "riscv32")]
fn uart_hex32(prefix: &str, v: u32) {
    uart_str(prefix);
    let hex = b"0123456789ABCDEF";
    const UART0_FIFO: *mut u32 = 0x500C_A000 as *mut u32;
    for i in 0..8 {
        unsafe {
            core::ptr::write_volatile(UART0_FIFO, hex[((v >> ((7 - i) * 4)) & 0xF) as usize] as u32);
        }
    }
    uart_str("\r\n");
}

#[cfg(target_arch = "riscv32")]
pub fn disable_all() {
    uart_str("wdt: disable_all entry\r\n");
    // SAFETY: peripheral pointers are static, single-hart at boot.
    unsafe {
        let lp = &*pac::LP_WDT::PTR;
        let cfg_before = lp.config0().read().bits();
        uart_hex32("  lp_wdt.config0 BEFORE = 0x", cfg_before);
        lp.wprotect().write(|w| w.bits(WDT_KEY));
        // Both `wdt_en` AND `wdt_flashboot_mod_en` must be cleared. ROM
        // sets `flashboot_mod_en=1` automatically when booting from flash
        // (see IDF `bootloader_init.c::bootloader_config_wdt`). Without
        // clearing it the chip resets ~1 s after handoff with reset
        // reason 0x09 (CORE_RTC_WDT) regardless of `wdt_en`.
        lp.config0().modify(|_, w| {
            w.wdt_en().clear_bit().wdt_flashboot_mod_en().clear_bit()
        });
        lp.wprotect().write(|w| w.bits(0));
        let cfg_after = lp.config0().read().bits();
        uart_hex32("  lp_wdt.config0 AFTER  = 0x", cfg_after);

        // SuperWDT — separate from LP_WDT main, gated by swd_wprotect + magic.
        // IDF `bootloader_super_wdt_auto_feed`: set swd_disable=1 (or auto-feed).
        let swd_before = lp.swd_config().read().bits();
        uart_hex32("  lp_wdt.swd_config BEFORE = 0x", swd_before);
        lp.swd_wprotect().write(|w| w.bits(WDT_KEY));
        // IDF `bootloader_super_wdt_auto_feed` sets ONLY auto_feed_en.
        // Setting swd_disable on top measured WORSE (0 ticks vs 1 tick) —
        // bit 30 has inverted semantics on P4. Stick with IDF approach.
        lp.swd_config().modify(|_, w| w.swd_auto_feed_en().set_bit());
        lp.swd_wprotect().write(|w| w.bits(0));
        let swd_after = lp.swd_config().read().bits();
        uart_hex32("  lp_wdt.swd_config AFTER  = 0x", swd_after);

        // TIMG0 MWDT — same flashboot dance. Reset reason 0x07
        // (CORE_MWDT0) is what fires if we miss this on a flash boot.
        let t0 = &*pac::TIMG0::PTR;
        let t0_before = t0.wdtconfig0().read().bits();
        uart_hex32("  timg0.wdtconfig0 BEFORE = 0x", t0_before);
        t0.wdtwprotect().write(|w| w.bits(WDT_KEY));
        t0.wdtconfig0().modify(|_, w| {
            w.wdt_en().clear_bit().wdt_flashboot_mod_en().clear_bit()
        });
        t0.wdtwprotect().write(|w| w.bits(0));
        let t0_after = t0.wdtconfig0().read().bits();
        uart_hex32("  timg0.wdtconfig0 AFTER  = 0x", t0_after);

        let t1 = &*pac::TIMG1::PTR;
        let t1_before = t1.wdtconfig0().read().bits();
        uart_hex32("  timg1.wdtconfig0 BEFORE = 0x", t1_before);
        t1.wdtwprotect().write(|w| w.bits(WDT_KEY));
        t1.wdtconfig0().modify(|_, w| {
            w.wdt_en().clear_bit().wdt_flashboot_mod_en().clear_bit()
        });
        t1.wdtwprotect().write(|w| w.bits(0));
        let t1_after = t1.wdtconfig0().read().bits();
        uart_hex32("  timg1.wdtconfig0 AFTER  = 0x", t1_after);
    }
    uart_str("wdt: disable_all done\r\n");
}

#[cfg(not(target_arch = "riscv32"))]
pub fn disable_all() {}
