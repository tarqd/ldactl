# Server-Sent Event Streams Codec

Implements a [`Codec`] for encoding and decoding [Server-Sent Events] streams.

Advantages:

- Minimizes allocations by using the buffer provided by [`FramedWrite`] and [`FramedRead`] while parsing lines
- Easy to use with the rest of the tokio ecosystem
- Can be used with any type that implements [`AsyncRead`] or [`AsyncWrite`]
- Errors implement [`miette::Diagnostic`] for better error and diagnostic messages

# Examples

```rust
use futures::StreamExt;
use tokio_util::codec::{FramedRead, Decoder};
use tokio_sse_codec::{SseDecoder, Frame, Event, SseDecodeError};

#[tokio::main]
async fn main() -> Result<(), SseDecodeError> {
// you can use any stream or type that implements `AsyncRead`
let data = "id: 1\nevent: example\ndata: hello, world\n\n";
let mut reader = FramedRead::new(data.as_bytes(), SseDecoder::<String>::new());

    while let Some(Ok(frame)) = reader.next().await {
    match frame {
        Frame::Event(event) => println!("event: id={:?}, name={}, data={}", event.id, event.name, event.data),
        Frame::Comment(comment) => println!("comment: {}", comment),
        Frame::Retry(duration) => println!("retry: {:#?}", duration),
    }
    }

    Ok::<(), SseDecodeError>(())
}
```

## Setting a buffer size limit

By default, the decoder will not limit the size of the buffer used to store the data of an event.
It's recommended to set one when dealing with untrusted input, otherwise a malicious server could send a very large event and consume all available memory.

The buffer should be able to hold a single event and it's data.

```rust
use tokio_sse_codec::SseDecoder;

let decoder  = SseDecoder::<String>::with_max_size(1024);
```
