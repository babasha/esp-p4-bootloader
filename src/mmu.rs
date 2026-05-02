//! MMU initialization. Mirrors IDF v5.3 `mmu_hal_init()` from
//! `components/hal/mmu_hal.c` for ESP32-P4.
//!
//! What it does on P4:
//! - `mmu_ll_set_page_size(0, MMU_PAGE_64KB)` is a runtime no-op (just an
//!   assert that the size is 64 KB — P4 has only one supported page size).
//! - `mmu_hal_unmap_all()` invalidates every entry of both MMU tables
//!   (flash via `SPI_MEM_C` MMU_ITEM_INDEX/CONTENT, PSRAM via `SPI_MEM_S`).
//! - `ROM_Boot_Cache_Init()` is **not** called: P4 does not advertise
//!   `ESP_ROM_RAM_APP_NEEDS_MMU_INIT` in `soc_caps`.
//!
//! Source: `idf_v53_ref/hal_esp32p4/include/hal/mmu_ll.h:329-362`,
//! `soc/spi_mem_c_reg.h:2627-2645`, `soc/spi_mem_s_reg.h:3387-3404`,
//! `soc/ext_mem_defs.h:59-88`.

#![allow(unsafe_code)]

use core::ptr;

/// `SOC_MMU_ENTRY_NUM` from `soc/ext_mem_defs.h:88`. Each MMU bank has
/// 1024 entries on ESP32-P4.
const MMU_ENTRY_NUM: u32 = 1024;

/// `SOC_MMU_FLASH_INVALID` / `SOC_MMU_PSRAM_INVALID` from `ext_mem_defs.h:59-61`.
/// On P4 both invalid encodings are zero — writing 0 to the content reg
/// marks the entry unmapped.
const MMU_INVALID: u32 = 0;

/// Flash MMU registers. `SPI_MEM_C_MMU_ITEM_INDEX_REG` /
/// `SPI_MEM_C_MMU_ITEM_CONTENT_REG` from `soc/spi_mem_c_reg.h`. Address
/// = `DR_REG_FLASH_SPI0_BASE` (`0x5008_C000`) + offset.
const FLASH_MMU_ITEM_INDEX_REG: *mut u32 = 0x5008_C380 as *mut u32;
const FLASH_MMU_ITEM_CONTENT_REG: *mut u32 = 0x5008_C37C as *mut u32;

/// PSRAM MMU registers. Same offsets but at `DR_REG_PSRAM_MSPI0_BASE`
/// (`0x5008_E000`). PSRAM_MSPI0 is the same controller our PSRAM code
/// pokes via `SPIMEM2_BASE`.
const PSRAM_MMU_ITEM_INDEX_REG: *mut u32 = 0x5008_E380 as *mut u32;
const PSRAM_MMU_ITEM_CONTENT_REG: *mut u32 = 0x5008_E37C as *mut u32;

/// `mmu_ll_unmap_all(mmu_id)` for one bank — write `0..ENTRY_NUM` to the
/// index reg and `INVALID` to the content reg, in lockstep. Mirrors
/// `idf_v53_ref/hal_esp32p4/include/hal/mmu_ll.h:357-362`.
#[inline(always)]
unsafe fn unmap_all_bank(index_reg: *mut u32, content_reg: *mut u32) {
    for i in 0..MMU_ENTRY_NUM {
        ptr::write_volatile(index_reg, i);
        ptr::write_volatile(content_reg, MMU_INVALID);
    }
}

/// Mirror of IDF `mmu_hal_init()` for ESP32-P4.
///
/// Resets the flash and PSRAM MMU tables to all-invalid. Page size is
/// fixed at 64 KB so `mmu_ll_set_page_size` collapses to a no-op.
pub fn hal_init() {
    // SAFETY: targets are fixed MMIO addresses; no concurrent accessor
    // (single-hart at boot, before IRQs / before `psram::detect`).
    unsafe {
        unmap_all_bank(FLASH_MMU_ITEM_INDEX_REG, FLASH_MMU_ITEM_CONTENT_REG);
        unmap_all_bank(PSRAM_MMU_ITEM_INDEX_REG, PSRAM_MMU_ITEM_CONTENT_REG);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cross-check the MMU register addresses against IDF defines.
    /// `DR_REG_HPPERIPH0_BASE = 0x5000_0000`, flash bank offset 0x8C000,
    /// psram bank offset 0x8E000, content reg offset 0x37C, index 0x380.
    #[test]
    fn mmu_reg_addresses() {
        assert_eq!(FLASH_MMU_ITEM_CONTENT_REG as usize, 0x5000_0000 + 0x8C000 + 0x37C);
        assert_eq!(FLASH_MMU_ITEM_INDEX_REG as usize, 0x5000_0000 + 0x8C000 + 0x380);
        assert_eq!(PSRAM_MMU_ITEM_CONTENT_REG as usize, 0x5000_0000 + 0x8E000 + 0x37C);
        assert_eq!(PSRAM_MMU_ITEM_INDEX_REG as usize, 0x5000_0000 + 0x8E000 + 0x380);
    }

    #[test]
    fn entry_count_matches_p4() {
        assert_eq!(MMU_ENTRY_NUM, 1024);
    }
}
