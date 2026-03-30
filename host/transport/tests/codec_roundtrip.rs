use holobridge_transport::{
    protocol::{ProtocolError, CONTROL_STREAM_CAPABILITY}, ControlMessage, ControlMessageCodec,
    FrameAccumulator, PROTOCOL_VERSION,
};

#[test]
fn hello_roundtrip_preserves_json_shape() {
    let message = ControlMessage::hello(
        "transport-smoke",
        vec![CONTROL_STREAM_CAPABILITY.to_owned()],
    );
    let encoded = ControlMessageCodec::encode(&message).expect("encode hello");
    let decoded = ControlMessageCodec::decode_frame(&encoded).expect("decode hello");

    assert_eq!(decoded, message);
}

#[test]
fn hello_ack_roundtrip_preserves_protocol_version() {
    let message = ControlMessage::hello_ack("ok");
    let encoded = ControlMessageCodec::encode(&message).expect("encode ack");
    let decoded = ControlMessageCodec::decode_frame(&encoded).expect("decode ack");

    assert_eq!(decoded.protocol_version(), Some(PROTOCOL_VERSION));
    assert_eq!(decoded, message);
}

#[test]
fn goodbye_roundtrip_works_in_accumulator() {
    let hello = ControlMessage::hello_smoke();
    let goodbye = ControlMessage::goodbye("client-close");
    let mut accumulator = FrameAccumulator::default();
    accumulator.push(&ControlMessageCodec::encode(&hello).expect("encode hello"));
    accumulator.push(&ControlMessageCodec::encode(&goodbye).expect("encode goodbye"));

    let messages = accumulator.drain_messages().expect("decode frames");
    assert_eq!(messages, vec![hello, goodbye]);
}

#[test]
fn malformed_frame_rejected() {
    let mut encoded = ControlMessageCodec::encode(&ControlMessage::hello_ack("ok"))
        .expect("encode malformed source");
    encoded[0..4].copy_from_slice(&999u32.to_be_bytes());

    let error = ControlMessageCodec::decode_frame(&encoded).expect_err("reject malformed frame");
    assert!(matches!(
        error,
        ProtocolError::LengthMismatch {
            declared: 999,
            actual: _
        }
    ));
}