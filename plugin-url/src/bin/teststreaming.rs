use std::error::Error;

use bytes::BytesMut;
use encoding_rs::{Encoding, CoderResult};
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut f = tokio::fs::File::open("/tmp/tmp.GfhYNq6JcO/big.txt").await?;
    let mut buffer = BytesMut::with_capacity(12);

    let read_count = f.read_buf(&mut buffer).await?;
    println!("read {read_count} bytes and the buffer is now: {buffer:?}");

    // add some complex codepoint
    let sparkle_heart = vec![240, 159, 146, 150];
    buffer.extend_from_slice(&sparkle_heart[..2]);

    println!("buffer is now: {buffer:?}");

    // buffer.extend_from_slice(&sparkle_heart[2..]);
    // println!("buffer is now: {buffer:?}");

    let reached_end_of_stream = read_count == 0;

    let mut dst = String::with_capacity(50);

    let mut decoder = Encoding::for_label(b"utf-8").unwrap().new_decoder();
    let (res, byte_read, did_replace) =
        decoder.decode_to_string(&buffer, &mut dst, reached_end_of_stream);

    println!("read {byte_read} from input buffer. did replace invalid garbage? {did_replace}");
    match res {
        CoderResult::InputEmpty => println!("input was empty!"),
        CoderResult::OutputFull => println!("output buffer is full"),
    }
    println!("decoded string is now:\n{dst}");

    Ok(())
}
