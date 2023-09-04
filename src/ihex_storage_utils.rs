pub const SEGMENT_BYTES: u16 = 8192;
pub const IHEX_SEGMENT_BYTES: u32 = 0x10000;

fn ibyte_to_mapbyte(ibyte: u16) -> (usize,u8) {
    ((ibyte / 8) as usize, (ibyte % 8) as u8)
}

/* Computes how many bits from the beginning and end, as well as the number of full bytes to read from a segment map */
pub fn get_pad_counts(start: u16, len: u16) -> (u16, u16, u16) {
    // The lower 8 byte boundary
    let start_align = (start / 8) * 8;
    // If unaligned compute how many bits of padding there are. If aligned it's 0
    let seg_bits_leading = (8 - (start - start_align)).min(len) % 8;
    let seg_full_bytes = (len - seg_bits_leading) / 8;
    let seg_bits_ending = len - seg_bits_leading - (seg_full_bytes * 8);
    (seg_bits_leading, seg_full_bytes, seg_bits_ending)
}

pub fn start_mask(bits: u8) -> u8 {
    if bits >= 8 {
        0xFF
    } else {
        let msk: u16 = (1 << bits) - 1;
        (msk & 0xFF) as u8
    }
}

pub fn end_mask(bits: u8) -> u8 {
    if bits > 8 {
        0xFF
    } else {
        (((0xFF as u16) << (8-bits)) & 0xFF) as u8
    }
}

fn bit_msk(num: u8) -> u8 {0b10000000 >> num}

/**
 * Fills the desired portion of a segment map, setting the correct bits to represent the equivlent bytes
 */
pub fn fill_bytes(map: &mut Vec<u8>, start: u16, len: u16) {
    /* Writes across a 64kb boundary wrap to the beginning of the same segment */
    let remainder = ((start as i32) + (len as i32) - 0x10000).max(0) as u16;
    let len = len - remainder;
    let (bits_leading, bytes_full, bits_ending) = get_pad_counts(start, len);
    let start_offset = if bits_leading == 0 {0} else {1};
    /* Leading bits don't always go to the byte boundary, 3 bytes for a byte like 0b00111000 are allowed */
    // I think the issue has to do with bit order. IE the first byte is represented as the least sig bit of the first byte, eg 00000001 instead of in reading order 10000000
    let (target_byte, target_bit) = ibyte_to_mapbyte(start);
    for i in 0..bits_leading {
        map[target_byte] |= bit_msk(i as u8 + target_bit);
    }

    /* Fill the full bytes */
    /* Note: Data records do cross segments but instead wrap around if offset + len > segment. TODO: Support this edge case */
    for i in 0..bytes_full {
        map[target_byte + start_offset + (i as usize)] = 0xFF;
    }

    /* Fill any tailing bits */
    if bits_ending != 0 {
        map[target_byte + start_offset + (bytes_full as usize)]  |= end_mask(bits_ending as u8);
    }

    /* Wrap around and fill any remaining bytes at the beginning */
    if remainder > 0 {
        fill_bytes(map, 0, remainder);
    }

}

pub fn is_seg_range_set(segment: &Vec<u8>, start: u16, len: u16) -> bool {
    // Convert the ihex byte range to bit ranges on the segment map
    let remainder = ((start as i32) + (len as i32) - 0x10000).max(0) as u16;
    let len = len - remainder;
    let (bits_leading, bytes_full, bits_ending) = get_pad_counts(start, len);
    let start_offset = if bits_leading == 0 {0} else {1};

    let (target_byte, target_bit) = ibyte_to_mapbyte(start);
    for i in 0..bits_leading {
        if segment[target_byte] & bit_msk(i as u8 + target_bit)  != 0 {
            return true;
        }
    }

    for i in 0..bytes_full {
        if segment[target_byte + start_offset + i as usize] != 0 {
            return true;
        }
    }

    if bits_ending > 0 && (segment[target_byte + start_offset + bytes_full as usize] & end_mask(bits_ending as u8)) != 0 {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::{ihex_storage_utils::{ibyte_to_mapbyte, get_pad_counts}, start_mask, end_mask, fill_bytes, is_seg_range_set};

    #[test]
    fn test_ibyte_to_mapbyte() -> Result<(),String> {
        assert_eq!((0,0), ibyte_to_mapbyte(0));
        assert_eq!((0,1), ibyte_to_mapbyte(1));
        assert_eq!((0,7), ibyte_to_mapbyte(7));
        assert_eq!((1,0), ibyte_to_mapbyte(8));
        Ok(())
    }

    /* A shorthand to add a total length in a redable way */
    fn _len(start: u16, full: u16, end: u16) -> u16 {start + full*8 + end}

    #[test]
    fn test_get_pad_counts() -> Result<(),String> {
        /* No length, no values */
        assert_eq!((0,0,0), get_pad_counts(0, 0));

        /* Inner byte ranges always report as start bits, it is the callers job to know where to put them */
        assert_eq!((1,0,0), get_pad_counts(0, 1));
        assert_eq!((7,0,0), get_pad_counts(0, 7));
        assert_eq!((1,0,0), get_pad_counts(1, 1));
        assert_eq!((6,0,0), get_pad_counts(1, 6));
        assert_eq!((1,0,0), get_pad_counts(15, 1));

        /* A full byte unaligned will properly reflect as a partial begin and end with no full */
        assert_eq!((7,0,1), get_pad_counts(1, 8));
        assert_eq!((4,0,4), get_pad_counts(4, 8));
        assert_eq!((1,0,7), get_pad_counts(7, 8));

        /* Perfect aligned bytes report no begin/end */
        assert_eq!((0,1,0), get_pad_counts(0, 8));
        assert_eq!((0,2,0), get_pad_counts(0, 16));

        /* Full bytes with a begin */
        assert_eq!((1,1,0), get_pad_counts(7, _len(1,1,0)));
        assert_eq!((7,1,0), get_pad_counts(1, _len(7,1,0)));
        assert_eq!((1,2,0), get_pad_counts(15, _len(1,2,0)));
        assert_eq!((7,2,0), get_pad_counts(9, _len(7,2,0)));

        /* Full bytes with an end */
        assert_eq!((0,1,1), get_pad_counts(0, _len(0,1,1)));
        assert_eq!((0,2,1), get_pad_counts(8, _len(0,2,1)));
        assert_eq!((0,2,7), get_pad_counts(8, _len(0,2,7)));

        /* Full bytes with begin and end */
        assert_eq!((1,1,1), get_pad_counts(7, _len(1,1,1)));
        assert_eq!((1,2,1), get_pad_counts(7, _len(1,2,1)));

        Ok(())
    }

    #[test]
    fn test_masks() -> Result<(),String> {
        assert_eq!(0, start_mask(0));
        assert_eq!(0b00000001, start_mask(1));
        assert_eq!(0b00000011, start_mask(2));
        assert_eq!(0b00000111, start_mask(3));
        assert_eq!(0b00001111, start_mask(4));
        assert_eq!(0b00011111, start_mask(5));
        assert_eq!(0b00111111, start_mask(6));
        assert_eq!(0b01111111, start_mask(7));
        assert_eq!(0b11111111, start_mask(8));
        assert_eq!(0b11111111, start_mask(255));

        assert_eq!(0, end_mask(0));
        assert_eq!(0b10000000, end_mask(1));
        assert_eq!(0b11000000, end_mask(2));
        assert_eq!(0b11100000, end_mask(3));
        assert_eq!(0b11110000, end_mask(4));
        assert_eq!(0b11111000, end_mask(5));
        assert_eq!(0b11111100, end_mask(6));
        assert_eq!(0b11111110, end_mask(7));
        assert_eq!(0b11111111, end_mask(8));
        assert_eq!(0b11111111, end_mask(255));

        Ok(())
    }

    #[test]
    fn test_fill_bytes() -> Result<(),String> {
        let mut test_vec: Vec<u8> = Vec::new();
        test_vec.resize(8192, 0);
        /* Keep everything in the first 128 bytes for readability. Test as many edge cases as posible since off-by-one style errors have been common */
        let expected_vec: Vec<u8> = [
            0xFF,0xFF,0xFF,0,0,0,0,0, //line 0, byte 0, len 24
            0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF, // line 1, byte 0, len 64
            0,0,0,0,0xFF,0,0,0, // line 2, byte 32, len 8
            0,0,0,0,0,0,0xFF,0xFF, //line 3, 40 len 16
            0b1,0xFF,0,0xFF,0b10000000,0xF,0xFF,0xF0, //line 4, byte 7 len 1 | byte 24, len 9 | byte 43 len 16
            0b10000000,0,0b00010000,0,0b00111100,0x0F,0,0, //line 5, byte 0 len 1 | byte 19, len 1 | byte 34 len 4 | byte 44 len 4
            0b1,0b10000000,0,0,0,0,0,0,
            0,0,0,0,0,0,0,0,
        ].to_vec();

        fill_bytes(&mut test_vec, 0, 24);
        fill_bytes(&mut test_vec, 64*1, 64);
        fill_bytes(&mut test_vec, 64*2 + 32, 8);
        fill_bytes(&mut test_vec, 64*3 + 48, 16);
        fill_bytes(&mut test_vec, 64*4 + 7, 9);
        fill_bytes(&mut test_vec, 64*4 + 24, 9);
        fill_bytes(&mut test_vec, 64*4 + 44, 16);
        fill_bytes(&mut test_vec, 64*5, 1);
        fill_bytes(&mut test_vec, 64*5 + 19, 1);
        fill_bytes(&mut test_vec, 64*5 + 34, 4);
        fill_bytes(&mut test_vec, 64*5 + 44, 4);
        fill_bytes(&mut test_vec, 64*6 + 7, 2);
        assert_eq!(expected_vec, test_vec[..expected_vec.len()]);

        Ok(())
    }

    #[test]
    fn test_is_seg_range_set() -> Result<(),String> {
        let mut test_vec: Vec<u8> = [
            0xFF,0xFF,0xFF,0,0,0,0,0, //line 0, byte 0, len 24
            0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF, // line 1, byte 0, len 64
            0,0,0,0,0xFF,0,0,0, // line 2, byte 32, len 8
            0,0,0,0,0,0,0xFF,0xFF, //line 3, 40 len 16
            0b1,0xFF,0,0xFF,0b10000000,0xF,0xFF,0xF0, //line 4, byte 7 len 1 | byte 24, len 9 | byte 43 len 16
            0b10000000,0,0b00010000,0,0b00111100,0x0F,0,0, //line 5, byte 0 len 1 | byte 19, len 1 | byte 34 len 4 | byte 44 len 4
            0b1,0b10000000,0,0,0,0,0,0,
            0,0,0,0,0,0,0,0,
        ].to_vec();
        test_vec.resize(8192, 0);

        /* Full vector tests */
        assert!(is_seg_range_set(&test_vec, 0, 255));
        assert!(is_seg_range_set(&test_vec, 0, 8192));
        assert!(is_seg_range_set(&test_vec, 0, 0xFFFF));

        /* Find empty ranges */
        /* Test after the boundary -- overflow bytes report as clear */
        assert!(!is_seg_range_set(&test_vec, 0x1000, 0xFFFF));
        assert!(!is_seg_range_set(&test_vec, 24, 1));
        assert!(!is_seg_range_set(&test_vec, 24, 32));
        assert!(!is_seg_range_set(&test_vec, 24, 32));
        assert!(!is_seg_range_set(&test_vec, 64*8*5+1, 8));
        assert!(!is_seg_range_set(&test_vec, 64*8*5+1, 18));
        assert!(!is_seg_range_set(&test_vec, 64*8*5+40, 4));

        /* Find populated ranges */
        assert!(is_seg_range_set(&test_vec, 0, 1));
        assert!(is_seg_range_set(&test_vec, 2, 1));
        assert!(is_seg_range_set(&test_vec, 8, 1));
        assert!(is_seg_range_set(&test_vec, 23, 1));
        assert!(is_seg_range_set(&test_vec, 23, 8));
        assert!(is_seg_range_set(&test_vec, 64*5+19, 1));
        assert!(is_seg_range_set(&test_vec, 64*5+19, 8));
        assert!(is_seg_range_set(&test_vec, 64*5+40, 5));
        assert!(is_seg_range_set(&test_vec, 64*6+7, 2));


        Ok(())
    }

}