//! REGI2C analog control bus.
//!
//! MPLL frequency configuration on ESP32-P4 goes via an internal I2C-like bus
//! to the analog block. Source: `idf_v53_ref/esp_rom_regi2c_p4.c` (the v5.3
//! patch — ROM has these stubs but P4 uses a software impl).
//!
//! Algorithm per write/read:
//!   1. Enable LP_I2CMST clock (LPPERI_CK_EN_LP_I2CMST bit).
//!   2. Select 160 MHz I2C master clock + select target block in CONF2.
//!   3. Wait for BUSY bit to clear.
//!   4. Compose CTRL = slave_id | (reg_addr << 8) | (wr_cntl << 24) | (data << 16).
//!   5. Write to CTRL register; engine sets BUSY=1, processes, BUSY=0.
//!   6. For reads, read DATA field (bits 16..23) after BUSY clears.

#![allow(unsafe_code)]

use core::ptr;

const I2C_ANA_MST_BASE: usize = 0x5012_4000;
const I2C_ANA_MST_I2C0_CTRL: *mut u32 = I2C_ANA_MST_BASE as *mut u32; // +0x00
const I2C_ANA_MST_ANA_CONF1: *mut u32 = (I2C_ANA_MST_BASE + 0x1c) as *mut u32;
const I2C_ANA_MST_ANA_CONF2: *mut u32 = (I2C_ANA_MST_BASE + 0x20) as *mut u32;
const I2C_ANA_MST_CLK160M: *mut u32 = (I2C_ANA_MST_BASE + 0x34) as *mut u32;
const I2C_ANA_MST_CLK_SEL_160M: u32 = 1 << 0;

const LPPERI_CLK_EN_REG: *mut u32 = 0x5012_0000 as *mut u32;
const LPPERI_CK_EN_LP_I2CMST: u32 = 1 << 27;

/// I2C target IDs (slaves) on the analog bus. Source:
/// `idf_v53_ref/esp_rom_regi2c_p4.c:58-80`.
pub const REGI2C_MSPI_BLOCK: u8 = 0x63;
pub const REGI2C_BIAS_BLOCK: u8 = 0x6A;

/// CONF2 master-select bits per block. Source:
/// `idf_v53_ref/esp_rom_regi2c_p4.c:21-28`.
pub const REGI2C_MSPI_XTAL_MST_SEL: u32 = 1 << 9;
pub const REGI2C_BIAS_MST_SEL: u32 = 1 << 12;

/// MPLL register addresses (within the MSPI I2C target). Source:
/// `idf_v53_ref/regi2c_mpll.h`.
pub const I2C_MPLL_IR_CAL_RSTB_ADDR: u8 = 1;
pub const I2C_MPLL_IR_CAL_RSTB_LSB: u8 = 5;
pub const I2C_MPLL_DIV_REG_ADDR: u8 = 2;
pub const I2C_MPLL_DHREF_ADDR: u8 = 3;
pub const I2C_MPLL_DHREF_LSB: u8 = 4;

const REGI2C_RTC_BUSY_BIT: u32 = 1 << 25;
const REGI2C_RTC_WR_CNTL_BIT: u32 = 1 << 24;
const REGI2C_RTC_DATA_SHIFT: u32 = 16;
const REGI2C_RTC_DATA_MASK: u32 = 0xFF;
const REGI2C_RTC_ADDR_SHIFT: u32 = 8;

#[inline(always)]
pub unsafe fn enable_bus() {
    let v = ptr::read_volatile(LPPERI_CLK_EN_REG);
    ptr::write_volatile(LPPERI_CLK_EN_REG, v | LPPERI_CK_EN_LP_I2CMST);
    let v = ptr::read_volatile(I2C_ANA_MST_CLK160M);
    ptr::write_volatile(I2C_ANA_MST_CLK160M, v | I2C_ANA_MST_CLK_SEL_160M);
    ptr::write_volatile(I2C_ANA_MST_ANA_CONF1, 0);
}

#[inline(always)]
pub unsafe fn select_block(mst_sel_bit: u32) {
    ptr::write_volatile(I2C_ANA_MST_ANA_CONF2, mst_sel_bit);
}

#[inline(always)]
pub unsafe fn wait_idle() {
    while ptr::read_volatile(I2C_ANA_MST_I2C0_CTRL) & REGI2C_RTC_BUSY_BIT != 0 {
        core::hint::spin_loop();
    }
}

/// Read full byte at `reg_addr` of slave `block`.
pub unsafe fn read_byte_block(block: u8, reg_addr: u8) -> u8 {
    wait_idle();
    let cmd = (block as u32) | ((reg_addr as u32) << REGI2C_RTC_ADDR_SHIFT);
    ptr::write_volatile(I2C_ANA_MST_I2C0_CTRL, cmd);
    wait_idle();
    let v = ptr::read_volatile(I2C_ANA_MST_I2C0_CTRL);
    ((v >> REGI2C_RTC_DATA_SHIFT) & REGI2C_RTC_DATA_MASK) as u8
}

/// Read full byte at `reg_addr` of the MSPI I2C target.
pub unsafe fn read_byte(reg_addr: u8) -> u8 {
    read_byte_block(REGI2C_MSPI_BLOCK, reg_addr)
}

/// Write full byte to `reg_addr` of slave `block`.
pub unsafe fn write_byte_block(block: u8, reg_addr: u8, data: u8) {
    wait_idle();
    let cmd = (block as u32)
        | ((reg_addr as u32) << REGI2C_RTC_ADDR_SHIFT)
        | REGI2C_RTC_WR_CNTL_BIT
        | (((data as u32) & REGI2C_RTC_DATA_MASK) << REGI2C_RTC_DATA_SHIFT);
    ptr::write_volatile(I2C_ANA_MST_I2C0_CTRL, cmd);
    wait_idle();
}

/// Write full byte to `reg_addr` of the MSPI I2C target.
pub unsafe fn write_byte(reg_addr: u8, data: u8) {
    write_byte_block(REGI2C_MSPI_BLOCK, reg_addr, data)
}

/// Read-modify-write a bit field [msb..lsb] in `reg_addr` of slave `block`.
pub unsafe fn set_field(block: u8, reg_addr: u8, msb: u8, lsb: u8, data: u8) {
    let cur = read_byte_block(block, reg_addr);
    let width = msb - lsb + 1;
    let mask = ((1u32 << width) - 1) as u8;
    let new = (cur & !(mask << lsb)) | ((data & mask) << lsb);
    write_byte_block(block, reg_addr, new);
}

/// Diagnostic: read the MPLL DIV register over REGI2C. Should be 0x99
/// (= (19<<3)|1) after a successful `mpll::bringup_400`.
pub unsafe fn read_mpll_div() -> u8 {
    select_block(REGI2C_MSPI_XTAL_MST_SEL);
    read_byte(I2C_MPLL_DIV_REG_ADDR)
}

/// Diagnostic: read the BIAS DREG_1P1 register. Top nibble should be 0xA
/// after `mpll::bringup_400` (IDF sets DREG_1P1 = 10).
pub unsafe fn read_bias_dreg_1p1() -> u8 {
    select_block(REGI2C_BIAS_MST_SEL);
    let v = read_byte_block(REGI2C_BIAS_BLOCK, 0);
    select_block(REGI2C_MSPI_XTAL_MST_SEL);
    v
}

#[cfg(test)]
mod tests {
    /// `set_field` mask math (computed without touching MMIO). Cross-check
    /// against the same expression we'd write by hand.
    #[test]
    fn set_field_mask_math() {
        // Field [7:4], data 10 → byte high nibble = 10.
        let cur: u8 = 0xA5; // 1010_0101
        let msb = 7u8;
        let lsb = 4u8;
        let data = 10u8;
        let width = msb - lsb + 1;
        let mask = ((1u32 << width) - 1) as u8;
        assert_eq!(mask, 0x0F);
        let shifted_mask = mask << lsb;
        assert_eq!(shifted_mask, 0xF0);
        let new = (cur & !shifted_mask) | ((data & mask) << lsb);
        assert_eq!(new, 0xA5); // 0xA was already there

        // Field [3:0], data 10 → byte low nibble = 10.
        let new = (cur & !0x0F) | (data & 0x0F);
        assert_eq!(new, 0xAA);
    }

    #[test]
    fn mpll_div_value_for_400_from_40() {
        // ref_div = 1, div = 400 / 20 - 1 = 19.
        let ref_div: u8 = 1;
        let div: u8 = (400u32 / 20 - 1) as u8;
        assert_eq!(div, 19);
        let val: u8 = (div << 3) | ref_div;
        assert_eq!(val, 0x99);
    }
}
