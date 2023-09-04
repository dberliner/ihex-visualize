use ihex::{Record,Reader};
use log::{debug, warn};
use std::collections::HashMap;
use std::fs;
use std::error::Error;
use clap::Parser;
use clap_num::maybe_hex;
use crossterm::{cursor, queue, style, execute, terminal,};
use std::io::{stdin, stdout, Read, Write};
mod ihex_storage_utils;
pub use crate::ihex_storage_utils::{*};

const CHR_BLANK: char = '░';
const CHR_DATA: char  = '▓';

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The Intel Hex file to process
    #[arg(short, long)]
    file: Option<String>,

    /// How many bytes each line represents (base 10 or hex)
    #[arg(short, long, value_parser=maybe_hex::<u16>, default_value_t = 0x1000)]
    line_width: u16,

    /// How many characters should be generated per line (base 10 or hex)
    #[arg(short, long, value_parser=maybe_hex::<u16>, default_value_t = 128)]
    display_width: u16,

    // Enable debug output
    #[arg(long, default_value_t = false)]
    debug: bool,
}

fn print_map_line(line: &Vec<bool>) {
    let mut line_str = String::with_capacity(line.len());
    for i in  line.into_iter() {line_str.push(if *i==false {CHR_BLANK} else {CHR_DATA})};

    queue!(
        stdout(),
        /* Move past the last column */
        cursor::MoveToColumn(10),
        style::Print(format!("{line_str}")),
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
    /* Get the hex file object */
    let args = Args::parse();
    let is_debug = args.debug;
    let width_symbols = args.display_width;
    let bytes_per_line = args.line_width ;
    let bytes_per_char = bytes_per_line / width_symbols;
    let bytes_per_char_rem = args.line_width % width_symbols;
    let map_start_xy: (u16, u16) = (0, 2);

    /* Init logging */
    let log_level = if is_debug {log::Level::Debug} else {log::Level::Warn};
    simple_logger::init_with_level(log_level).unwrap();

    if bytes_per_char_rem > 0 {
        warn!("The requested line width of {bytes_per_line} cannot be divided evenly across {width_symbols} \
               characters. All characters will represent {bytes_per_char} characters except the last symbol \
               of each line, which will represent {bytes_per_char_rem}.");
    }

    // TODO Support multiple segments per line
    if IHEX_SEGMENT_BYTES % bytes_per_line as u32 != 0 {
        warn!("Segments of {IHEX_SEGMENT_BYTES} cannot be evenly represented in {bytes_per_line} byte lines. Insufficient lines will be 0-filled.")
    }

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
                /* Determine wich part of the segment map we need to access. ESX can offset in or between pages. */
                let (page, esx_offset) = if ihex_esx_addr != 0 {
                    ((ihex_esx_addr & 0xF000)>>12 as u16, ihex_esx_addr*16)
                } else {
                    (ihex_ela_addr, 0)
                };

                /* Find the segment or create it if it doesn't exist. */
                segment_map.entry(page)
                    .or_default()
                    .resize(SEGMENT_BYTES as usize, 0);

                /* Fill the proper bits in this segment */
                fill_bytes(
                    segment_map.get_mut(&page).expect("Could not find EXS"),
                    offset + esx_offset,
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
    let last_seg_idx = *seg_idxs.last().expect("Could not get last segment");

    /* Fill in the address data */
    let max_addr: u32 = (last_seg_idx as u32 + 1) * (SEGMENT_BYTES as u32) * 8 - 1;
    let hex_width = (std::format!("{:#01x}", max_addr as u32).len() & 0xFF) as u8;
    let lines_per_seg = IHEX_SEGMENT_BYTES / bytes_per_line as u32;
    let lines_total = (max_addr + 1) / bytes_per_line as u32;

    /* Write the data onto an alternatie screen */
    execute!(stdout(), terminal::EnterAlternateScreen)?;
    queue!(
        stdout(),
        cursor::MoveTo(0, 0),
        style::Print(format!("Printing out segment map with bytes_per_line={bytes_per_line} bytes_per_char={bytes_per_char} hex_width={hex_width} lines_per_seg={lines_per_seg} lines_total={lines_total}")),
        cursor::MoveToNextLine(2)
    )?;
    stdout().flush().expect("Could not flush");

    // Fill in the addresses on the left
    fill_map_addrs(map_start_xy, lines_total, 10, hex_width, bytes_per_line, 0);

    /* Print the actual map */
    for seg_idx in 0..last_seg_idx+1 {
        match segment_map.get(&seg_idx) {
            Some(segment) => {
                for line_num in 0..lines_per_seg {
                    let mut line_data: Vec<bool> = Vec::new();
    
                    for chr in 0..width_symbols {
                        // The requested number of bytes plus the remainder at the end if asked for a nondivisible combination
                        let is_last = chr==width_symbols-1;
                        let num_bytes = bytes_per_char+{if is_last {bytes_per_char_rem} else {0}};
                        // The offset in the segment
                        let ihex_start_byte = bytes_per_line * line_num as u16 + chr * bytes_per_char;
                        let res = is_seg_range_set(
                            &segment,
                            ihex_start_byte,
                            num_bytes
                        );
                        line_data.push(res);
                        if res {
                            //println!("is_last={is_last} num_bytes={num_bytes} ihex_start_byte={ihex_start_byte} res={res}");
                        }
                    }
    
                    print_map_line(&line_data);
                }
            },
            None => {
                let mut line_data: Vec<bool> = Vec::new();
                line_data.resize(width_symbols as usize, false);
                print_map_line(&line_data);
            },
        };
    }
    //println!("{:?}",segment_map.get(&0).expect("Could not get segment 0"));

    /* Pause and exit */
    pause();
    execute!(stdout(), terminal::LeaveAlternateScreen)?;
    Ok(())
}
