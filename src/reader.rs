use std::{
    io::{BufReader, Read},
    thread,
};

use crossbeam_channel::Sender;

pub fn start_reader<R: Read + Send + 'static>(
    mut reader: BufReader<R>,
    read_tx: Sender<(usize, Vec<u8>)>,
    write_tx: Sender<(usize, Vec<u8>)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let buf_size = 65536; // 64 KiB

        let mut current_buffer = vec![0u8; buf_size];
        let mut chunk_index = 0;

        let initial_read_size = match reader.read(&mut current_buffer) {
            Ok(0) => return, // Handle empty file case
            Ok(size) => size,
            Err(e) => {
                eprintln!("Error reading file: {}", e);
                return;
            }
        };

        // Check for header (if the first newline isn't at the 82nd position, we have a header)
        let first_newline_pos = current_buffer
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(initial_read_size);

        let mut next_buffer = if first_newline_pos != 81 {
            let header = current_buffer[..first_newline_pos + 1].to_vec();
            write_tx
                .send((chunk_index, header))
                .expect("Error sending header to writer");
            chunk_index += 1;
            current_buffer[first_newline_pos + 1..initial_read_size].to_vec()
        } else {
            current_buffer[..initial_read_size].to_vec()
        };

        loop {
            let read_size = match reader.read(&mut current_buffer) {
                Ok(0) => break, // End of file reached.
                Ok(size) => size,
                Err(e) => {
                    eprintln!("Error reading file: {}", e);
                    return;
                }
            };

            // Append any carry-over from the previous iteration
            let mut data = if !next_buffer.is_empty() {
                let mut temp = std::mem::take(&mut next_buffer);
                temp.extend_from_slice(&current_buffer[..read_size]);
                temp
            } else {
                current_buffer[..read_size].to_vec()
            };

            // Find the last newline to determine the carry-over for the next chunk
            if let Some(last_newline_pos) = data.iter().rposition(|&b| b == b'\n') {
                next_buffer = data[last_newline_pos + 1..].to_vec();
                data.truncate(last_newline_pos + 1);
            } else {
                next_buffer.clear();
            }

            if read_tx.send((chunk_index, data)).is_err() {
                eprintln!("Error sending chunk to workers");
                return;
            }
            chunk_index += 1;

            // Prepare the current buffer for the next read, adjusting the size if necessary
            current_buffer.resize(buf_size, 0);
        }

        // Handle any remaining data as carry-over
        if !next_buffer.is_empty() {
            if read_tx.send((chunk_index, next_buffer)).is_err() {
                eprintln!("Error sending final carry-over chunk to workers");
            }
        }
    })
}
