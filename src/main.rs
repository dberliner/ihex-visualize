use ihex::{Record,Reader};
use std::collections::HashMap;
use std::fs;
use std::error::Error;
use clap::Parser;
use log::{info, debug, error};
use simple_logger::SimpleLogger;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The Intel Hex file to process
    #[arg(short, long)]
    file: Option<String>,

    /// Number of bytes per character. Currently supported are values are 2^n for n∈[3,11] (so 8, 16 ... 2048)
    #[arg(short, long, default_value_t = 64)]
    density_bytes: u16,

    /// Number of characters per line
    #[arg(short, long, default_value_t = 128)]
    width_symbols: u16,

    // Print the first (zero-th) page instead of the map
    #[arg(long, default_value_t = false)]
    print_vector: bool,

    // Enable debug output
    #[arg(long, default_value_t = false)]
    debug: bool,

}

fn ibyte_to_mapbyte(ibyte: u16) -> usize {
    (ibyte / 8) as usize
}

fn fill_bytes(map: &mut Vec<u8>, start: u16, len: u16) {
    /* Writes across a 64kb boundary wrap to the beginning of the same segment */
    let remainder = ((start as i32) + (len as i32) - 0x10000).max(0) as u16;
    let len = len - remainder;

    let bits_leading = {
        let len_or_8 = len.min(8);
        /* If start is alligned there are no leading bits */
        if start % len_or_8 == 0 {0}
        /* Otherwise there are between 1 and 7 */
        else {len_or_8 - (start % 8)}
    };
    let bits_ending = (len - bits_leading) % 8;
    let bytes_full = (len - bits_leading - bits_ending) / 8;
    debug!("This row is {bytes_full} bytes with leading={bits_leading} and tailing={bits_ending}");

    /* Fill bits up to the byte boundary */
    let mut target_byte = ibyte_to_mapbyte(start);
    if bits_leading != 0 {
        map[target_byte] |= (1<<bits_leading)-1;
        target_byte += 1;
    }

    /* Fill the full bytes */
    /* Note: Data records do cross segments but instead wrap around if offset + len > segment. TODO: Support this edge case */
    for i in 0..bytes_full {
        map[target_byte + (i as usize)] = 0xFF;
    }

    /* Fill any tailing bits */
    if bits_ending != 0 {
        map[target_byte + (bytes_full as usize)]  |= !((1<<(8 - bits_ending))-1);
    }

    /* Wrap around and fill any remaining bytes at the beginning */
    if remainder > 0 {
        fill_bytes(map, 0, remainder);
    }

}


fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let chr_blank = "░";
    let chr_data  = "▓";
    let file_str = fs::read_to_string(args.file.expect("Memory error for args.file"))?;
    let mut ihex_ela_addr: u16 = 0;
    let mut ihex_esx_addr: u16 = 0;
    let reader = Reader::new(&file_str);
    let segment_width_bytes: u16 = 8192;

    let mut segment_map: HashMap<u16, Vec<u8>> = HashMap::new();
    let log_level = if args.debug {log::Level::Debug} else {log::Level::Warn};

    simple_logger::init_with_level(log_level).unwrap();

    for line in reader.into_iter() {
       match line {
        Ok(v) => match v {
            Record::Data { offset, value } => {
                let page = if ihex_esx_addr != 0 {
                    (ihex_esx_addr & 0xF000)>>12 as u16
                } else {
                    ihex_ela_addr
                };
                let page_offset = if ihex_esx_addr != 0 {
                    offset + (ihex_ela_addr & 0xFFF)
                } else {
                    offset
                };
                /* Starting address in page */
                debug!(
                    "I see a data row for page {page}+{offset} with a computed starting addr of {page_offset} and len of {}",
                    value.len());

                /* Find the segment or create it if it doesn't exist. */
                segment_map.entry(page)
                    .or_default()
                    .resize(segment_width_bytes as usize, 0);

                /* Fill the proper bits in this segment */
                fill_bytes(
                    segment_map.get_mut(&page).expect("Could not find EXS"),
                    offset,
                    value.len() as u16);
            },
            Record::ExtendedSegmentAddress(addr) => { ihex_esx_addr = addr; ihex_ela_addr = 0; },
            Record::ExtendedLinearAddress(addr)  => { ihex_esx_addr = 0; ihex_ela_addr = addr; },
            _ => {}, /* Other types not useful for this analysis */
        }
        Err(_) => {},
       }
    }

    if args.print_vector == false {
        /* Process the keys in order */
        let mut seg_idxs: Vec<u16> = segment_map
            .keys()
            .cloned()
            .collect();
        seg_idxs.sort();
        
        /* Line wraps don't have to align to segment boundaries so keep an independent tracker */
        let mut print_cnt = 0;
        /* The segment vector stores one byte per bit, so whatever the client is asked for should be divided by 8 */
        let seg_vec_density = args.density_bytes / 8 as u16;
        for seg_addr in 0..*seg_idxs.last().expect("Could not get last segment")+1 {
            match segment_map.get(&seg_addr) {
                Some(segment) => {
                    for chr in 0..(segment_width_bytes/seg_vec_density) {
                        let mut acc = false;
                        for i in 0..seg_vec_density {
                            if segment[(chr*seg_vec_density + i) as usize] != 0 {
                                acc = true;
                                break;
                            }
                        }
                        print_cnt = print_cnt + 1;
                        if acc {print!("{chr_data}");} else {print!("{chr_blank}");}
                        if print_cnt % args.width_symbols == 0 {println!("|"); print_cnt = 0;}
                    }
                },
                None => {
                    for _ in 0..(segment_width_bytes/seg_vec_density) {
                        print_cnt = print_cnt + 1;
                        print!("{chr_undef}");
                        if print_cnt % args.width_symbols == 0 {println!("|"); print_cnt = 0;}
                    }
                },
            };
        }
        println!("|");
    } else {
        println!("{:?}", segment_map[&0]);
    }
    Ok(())
}
