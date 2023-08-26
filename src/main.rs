use ihex::{Record,Reader};
use log::debug;
use std::collections::HashMap;
use std::fs;
use std::error::Error;
use clap::Parser;
use crossterm::{cursor, queue, style, execute, terminal,};
use std::io::{stdin, stdout, Read, Write};

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

    /// Count sequences of length N of 0xFF or 0x00 set bytes as unset (default 0=off)
    #[arg(long, default_value_t = 0)]
    explicit_undef: u16,

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

/**
 * Fills the desired portion of a segment map, setting the correct bits to represent the equivlent bytes
 */
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

fn print_map_line(symbols: &String) {
    let mut stdout = stdout();
    queue!(
        stdout,
        /* Actual values will be filled later */
        cursor::MoveToColumn(10),
        style::Print(format!("{symbols}")),
        cursor::MoveToNextLine(1),
    ).expect("Couldnt output line");
}

fn fill_map_addrs(start_xy: (u16, u16), lines: u32, bracket_width: u8, hex_width: u8, line_size: u16, initial_offset: u16) {
    let mut stdout = stdout();
    queue!(stdout, cursor::SavePosition).expect("Couldnt save cursor");
    for i in 0..lines {
        let addr = i * line_size as u32 + (if i==0 {initial_offset as u32} else {0});
        queue!(
            stdout,
            cursor::MoveTo(start_xy.0, start_xy.1 + i as u16),
            /* Print a hex value of the desired length for the address */
            style::Print(format!(
                "{:<bracket_width$}",
                format!(
                    "{:#0hex_width$x}",
                    addr,
                    hex_width=hex_width as usize),
                bracket_width=bracket_width as usize
            )),
        ).expect("Couldnt output line");
    }
    queue!(stdout, cursor::RestorePosition).expect("Couldnt reset cursor");
}

fn pause() {
    let mut stdout = stdout();
    queue!(
        stdout,
        style::Print("Press Enter to exit"),
        cursor::MoveToNextLine(2)
    ).expect("Could not output");
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}

fn main() -> Result<(), Box<dyn Error>> {
    const CHR_BLANK: char = '░';
    const CHR_DATA: char  = '▓';
    const SEGMENT_BYTES: u16 = 8192;

    /* Get the hex file object */
    let args = Args::parse();

    let map_start_xy: (u16, u16) = (0, 2);
    let mut line_cnt = 0;

    /* Init logging */
    let log_level = if args.debug {log::Level::Debug} else {log::Level::Warn};
    simple_logger::init_with_level(log_level).unwrap();

    /* A counter must be kept between rows to indicate address offsets. Only one of these will ever be set at a time.gitf */
    let mut ihex_ela_addr: u16 = 0;
    let mut ihex_esx_addr: u16 = 0;

    /* Store a map of every byte in the hex file, 0 if unset and 1 if set
       8kb (mapping 64kb) segments are added on-demand to minimize memory usage */
    let mut segment_map: HashMap<u16, Vec<u8>> = HashMap::new();

    /* Get the hex file contents as a (ihex) Reader object */
    let file_path = args.file.expect("Could not get file arg");
    let file_contents = fs::read_to_string(file_path).expect("Could not read file");
    let ihex_obj = Reader::new(&file_contents).into_iter();

    for line in ihex_obj {
       match line {
        Ok(v) => match v {
            Record::Data { offset, value } => {
                /* Determine wich part of the segment map we need to access */
                let page = if ihex_esx_addr != 0 {
                    (ihex_esx_addr & 0xF000)>>12 as u16
                } else {
                    ihex_ela_addr
                };

                /* Find the segment or create it if it doesn't exist. */
                segment_map.entry(page)
                    .or_default()
                    .resize(SEGMENT_BYTES as usize, 0);

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

    /* Process the keys in order */
    let mut seg_idxs: Vec<u16> = segment_map
        .keys()
        .cloned()
        .collect();
    seg_idxs.sort();

    /* The segment vector stores one byte per bit, so whatever the client is asked for should be divided by 8 */
    let seg_vec_density = args.density_bytes / 8 as u16;
    let mut line_str = "".to_owned();
    let last_seg_idx = *seg_idxs.last().expect("Could not get last segment");

    /* Fill in the address data */
    let max_addr: u32 = (last_seg_idx as u32 + 1) * (SEGMENT_BYTES as u32) * 8 - 1;
    let hex_width = (std::format!("{:#01x}", max_addr as u32).len() & 0xFF) as u8;
    let line_bytes = args.density_bytes * args.width_symbols;
    let lines = (max_addr + 1) / line_bytes as u32;

    /* Write the data onto an alternatie screen */
    execute!(stdout(), terminal::EnterAlternateScreen)?;
    queue!(
        stdout(),
        cursor::MoveTo(0, 0),
        style::Print("Printing out segment map"),
        cursor::MoveToNextLine(2)
    )?;
    stdout().flush().expect("Could not flush");

    /* Fill in the addresses on the left */
    fill_map_addrs(map_start_xy, lines as u32, 10, hex_width, line_bytes, 0);

    /* Print the actual map */
    for seg_addr in 0..last_seg_idx+1 {
        match segment_map.get(&seg_addr) {
            Some(segment) => {
                for chr in 0..(SEGMENT_BYTES/seg_vec_density) {
                    let mut any_1s = false;
                    for i in 0..seg_vec_density {
                        if segment[(chr*seg_vec_density + i) as usize] != 0 {
                            any_1s = true;
                            break;
                        }
                    }
                    line_str.push(if any_1s {CHR_DATA} else {CHR_BLANK});

                    if line_str.chars().count() == args.width_symbols as usize {
                        print_map_line(&line_str);
                        line_cnt = line_cnt + 1;
                        line_str = "".to_owned();
                    }
                }
            },
            None => {
                for _ in 0..(SEGMENT_BYTES/seg_vec_density) {
                    line_str.push(CHR_BLANK);
                    if line_str.chars().count() == args.width_symbols as usize {
                        print_map_line(&line_str);
                        line_cnt = line_cnt + 1;
                        line_str = "".to_owned();
                    }
                }
            },
        };
    }

    /* Pause and exit */
    pause();
    execute!(stdout(), terminal::LeaveAlternateScreen)?;
    Ok(())
}
