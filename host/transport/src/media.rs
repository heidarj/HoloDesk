use std::{
    collections::HashMap,
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use bytes::Bytes;
use holobridge_encode::EncodedAccessUnit;

pub const VIDEO_DATAGRAM_CAPABILITY: &str = "video-datagram-h264-v1";
pub const MEDIA_DATAGRAM_VERSION: u8 = 1;
pub const MEDIA_DATAGRAM_HEADER_LEN: usize = 32;
const KEYFRAME_FLAG: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDatagramHeader {
    pub access_unit_id: u64,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub pts_100ns: i64,
    pub duration_100ns: i64,
    pub is_keyframe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassembledAccessUnit {
    pub access_unit_id: u64,
    pub data: Vec<u8>,
    pub pts_100ns: i64,
    pub duration_100ns: i64,
    pub is_keyframe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReassemblerConfig {
    pub incomplete_timeout: Duration,
    pub max_in_flight_access_units: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReassemblerStats {
    pub dropped_incomplete_access_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaDatagramError {
    HeaderTooShort { actual: usize },
    UnsupportedVersion { actual: u8 },
    InvalidFragmentCount { actual: u16 },
    InvalidFragmentIndex { index: u16, count: u16 },
    EmptyPayload { access_unit_id: u64 },
    FragmentCountTooLarge { actual: usize },
    AccessUnitPayloadEmpty,
    MaxPayloadTooSmall { actual: usize },
    InconsistentFragmentMetadata { access_unit_id: u64 },
}

pub struct H264DatagramPacketizer {
    next_access_unit_id: u64,
}

pub struct H264DatagramReassembler {
    config: ReassemblerConfig,
    stats: ReassemblerStats,
    incomplete: HashMap<u64, IncompleteAccessUnit>,
}

#[derive(Debug)]
struct IncompleteAccessUnit {
    header: MediaDatagramHeader,
    first_seen_at: Instant,
    fragments: Vec<Option<Vec<u8>>>,
    received_fragments: usize,
}

impl Default for H264DatagramPacketizer {
    fn default() -> Self {
        Self {
            next_access_unit_id: 1,
        }
    }
}

impl Default for ReassemblerConfig {
    fn default() -> Self {
        Self {
            incomplete_timeout: Duration::from_millis(500),
            max_in_flight_access_units: 32,
        }
    }
}

impl MediaDatagramHeader {
    pub fn encode(&self) -> Result<[u8; MEDIA_DATAGRAM_HEADER_LEN], MediaDatagramError> {
        validate_fragment_shape(self.fragment_index, self.fragment_count)?;

        let mut encoded = [0u8; MEDIA_DATAGRAM_HEADER_LEN];
        encoded[0] = MEDIA_DATAGRAM_VERSION;
        encoded[1] = if self.is_keyframe { KEYFRAME_FLAG } else { 0 };
        encoded[4..12].copy_from_slice(&self.access_unit_id.to_be_bytes());
        encoded[12..14].copy_from_slice(&self.fragment_index.to_be_bytes());
        encoded[14..16].copy_from_slice(&self.fragment_count.to_be_bytes());
        encoded[16..24].copy_from_slice(&self.pts_100ns.to_be_bytes());
        encoded[24..32].copy_from_slice(&self.duration_100ns.to_be_bytes());
        Ok(encoded)
    }

    pub fn decode(datagram: &[u8]) -> Result<(Self, &[u8]), MediaDatagramError> {
        if datagram.len() < MEDIA_DATAGRAM_HEADER_LEN {
            return Err(MediaDatagramError::HeaderTooShort {
                actual: datagram.len(),
            });
        }

        let version = datagram[0];
        if version != MEDIA_DATAGRAM_VERSION {
            return Err(MediaDatagramError::UnsupportedVersion { actual: version });
        }

        let fragment_index = u16::from_be_bytes([datagram[12], datagram[13]]);
        let fragment_count = u16::from_be_bytes([datagram[14], datagram[15]]);
        validate_fragment_shape(fragment_index, fragment_count)?;

        let header = Self {
            access_unit_id: u64::from_be_bytes(datagram[4..12].try_into().expect("header access unit id")),
            fragment_index,
            fragment_count,
            pts_100ns: i64::from_be_bytes(datagram[16..24].try_into().expect("header pts")),
            duration_100ns: i64::from_be_bytes(datagram[24..32].try_into().expect("header duration")),
            is_keyframe: (datagram[1] & KEYFRAME_FLAG) != 0,
        };
        Ok((header, &datagram[MEDIA_DATAGRAM_HEADER_LEN..]))
    }
}

impl H264DatagramPacketizer {
    pub fn packetize(
        &mut self,
        access_unit: &EncodedAccessUnit,
        max_payload_bytes: usize,
    ) -> Result<Vec<Bytes>, MediaDatagramError> {
        self.packetize_bytes(
            &access_unit.data,
            access_unit.is_keyframe,
            access_unit.pts_100ns,
            access_unit.duration_100ns,
            max_payload_bytes,
        )
    }

    pub fn packetize_bytes(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_100ns: i64,
        duration_100ns: i64,
        max_payload_bytes: usize,
    ) -> Result<Vec<Bytes>, MediaDatagramError> {
        if data.is_empty() {
            return Err(MediaDatagramError::AccessUnitPayloadEmpty);
        }
        if max_payload_bytes == 0 {
            return Err(MediaDatagramError::MaxPayloadTooSmall { actual: 0 });
        }

        let fragment_count = data.len().div_ceil(max_payload_bytes);
        let fragment_count_u16 =
            u16::try_from(fragment_count).map_err(|_| MediaDatagramError::FragmentCountTooLarge {
                actual: fragment_count,
            })?;

        let access_unit_id = self.next_access_unit_id;
        self.next_access_unit_id = self.next_access_unit_id.saturating_add(1);

        let mut datagrams = Vec::with_capacity(fragment_count);
        for fragment_index in 0..fragment_count {
            let start = fragment_index * max_payload_bytes;
            let end = (start + max_payload_bytes).min(data.len());
            let payload = &data[start..end];
            let header = MediaDatagramHeader {
                access_unit_id,
                fragment_index: fragment_index as u16,
                fragment_count: fragment_count_u16,
                pts_100ns,
                duration_100ns,
                is_keyframe,
            };
            let header_bytes = header.encode()?;
            let mut datagram =
                Vec::with_capacity(MEDIA_DATAGRAM_HEADER_LEN + payload.len());
            datagram.extend_from_slice(&header_bytes);
            datagram.extend_from_slice(payload);
            datagrams.push(Bytes::from(datagram));
        }

        Ok(datagrams)
    }
}

impl H264DatagramReassembler {
    pub fn new(config: ReassemblerConfig) -> Self {
        Self {
            config,
            stats: ReassemblerStats::default(),
            incomplete: HashMap::new(),
        }
    }

    pub fn stats(&self) -> ReassemblerStats {
        self.stats
    }

    pub fn prune_expired(&mut self, now: Instant) {
        let timeout = self.config.incomplete_timeout;
        self.incomplete.retain(|_, unit| {
            let keep = now.saturating_duration_since(unit.first_seen_at) < timeout;
            if !keep {
                self.stats.dropped_incomplete_access_units += 1;
            }
            keep
        });
    }

    pub fn push_datagram(
        &mut self,
        datagram: &[u8],
        now: Instant,
    ) -> Result<Option<ReassembledAccessUnit>, MediaDatagramError> {
        self.prune_expired(now);

        let (header, payload) = MediaDatagramHeader::decode(datagram)?;
        if payload.is_empty() {
            return Err(MediaDatagramError::EmptyPayload {
                access_unit_id: header.access_unit_id,
            });
        }

        if header.fragment_count == 1 {
            return Ok(Some(ReassembledAccessUnit {
                access_unit_id: header.access_unit_id,
                data: payload.to_vec(),
                pts_100ns: header.pts_100ns,
                duration_100ns: header.duration_100ns,
                is_keyframe: header.is_keyframe,
            }));
        }

        while self.incomplete.len() >= self.config.max_in_flight_access_units {
            if let Some(oldest_id) = self
                .incomplete
                .iter()
                .min_by_key(|(_, unit)| unit.first_seen_at)
                .map(|(access_unit_id, _)| *access_unit_id)
            {
                self.incomplete.remove(&oldest_id);
                self.stats.dropped_incomplete_access_units += 1;
            } else {
                break;
            }
        }

        let entry = self.incomplete.entry(header.access_unit_id).or_insert_with(|| {
            IncompleteAccessUnit {
                first_seen_at: now,
                fragments: vec![None; header.fragment_count as usize],
                received_fragments: 0,
                header: header.clone(),
            }
        });

        if entry.header.fragment_count != header.fragment_count
            || entry.header.pts_100ns != header.pts_100ns
            || entry.header.duration_100ns != header.duration_100ns
            || entry.header.is_keyframe != header.is_keyframe
        {
            return Err(MediaDatagramError::InconsistentFragmentMetadata {
                access_unit_id: header.access_unit_id,
            });
        }

        let fragment_slot = &mut entry.fragments[header.fragment_index as usize];
        if fragment_slot.is_none() {
            *fragment_slot = Some(payload.to_vec());
            entry.received_fragments += 1;
        }

        if entry.received_fragments != entry.fragments.len() {
            return Ok(None);
        }

        let completed = self
            .incomplete
            .remove(&header.access_unit_id)
            .expect("reassembler entry must exist");
        let mut data = Vec::new();
        for fragment in completed.fragments {
            if let Some(fragment) = fragment {
                data.extend_from_slice(&fragment);
            }
        }

        Ok(Some(ReassembledAccessUnit {
            access_unit_id: header.access_unit_id,
            data,
            pts_100ns: completed.header.pts_100ns,
            duration_100ns: completed.header.duration_100ns,
            is_keyframe: completed.header.is_keyframe,
        }))
    }
}

pub fn negotiated_datagram_payload_limit(
    peer_max_datagram_size: Option<usize>,
    configured_payload_cap: usize,
) -> Option<usize> {
    let peer_max_datagram_size = peer_max_datagram_size?;
    if configured_payload_cap == 0 || peer_max_datagram_size <= MEDIA_DATAGRAM_HEADER_LEN {
        return None;
    }

    Some(
        configured_payload_cap.min(peer_max_datagram_size - MEDIA_DATAGRAM_HEADER_LEN),
    )
}

fn validate_fragment_shape(
    fragment_index: u16,
    fragment_count: u16,
) -> Result<(), MediaDatagramError> {
    if fragment_count == 0 {
        return Err(MediaDatagramError::InvalidFragmentCount { actual: fragment_count });
    }
    if fragment_index >= fragment_count {
        return Err(MediaDatagramError::InvalidFragmentIndex {
            index: fragment_index,
            count: fragment_count,
        });
    }
    Ok(())
}

impl fmt::Display for MediaDatagramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderTooShort { actual } => {
                write!(formatter, "media datagram shorter than header: {actual} bytes")
            }
            Self::UnsupportedVersion { actual } => {
                write!(formatter, "unsupported media datagram version: {actual}")
            }
            Self::InvalidFragmentCount { actual } => {
                write!(formatter, "invalid fragment count: {actual}")
            }
            Self::InvalidFragmentIndex { index, count } => {
                write!(
                    formatter,
                    "fragment index {index} is outside fragment count {count}"
                )
            }
            Self::EmptyPayload { access_unit_id } => {
                write!(formatter, "media datagram payload is empty for access unit {access_unit_id}")
            }
            Self::FragmentCountTooLarge { actual } => {
                write!(formatter, "access unit produced too many fragments: {actual}")
            }
            Self::AccessUnitPayloadEmpty => {
                formatter.write_str("encoded access unit payload is empty")
            }
            Self::MaxPayloadTooSmall { actual } => {
                write!(formatter, "max datagram payload is too small: {actual}")
            }
            Self::InconsistentFragmentMetadata { access_unit_id } => {
                write!(
                    formatter,
                    "fragment metadata changed within access unit {access_unit_id}"
                )
            }
        }
    }
}

impl Error for MediaDatagramError {}

#[cfg(test)]
mod tests {
    use super::{
        negotiated_datagram_payload_limit, H264DatagramPacketizer, H264DatagramReassembler,
        MediaDatagramHeader, MediaDatagramError, ReassemblerConfig, MEDIA_DATAGRAM_HEADER_LEN,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn header_roundtrip_preserves_fields() {
        let header = MediaDatagramHeader {
            access_unit_id: 42,
            fragment_index: 2,
            fragment_count: 7,
            pts_100ns: 123_456,
            duration_100ns: 16_666,
            is_keyframe: true,
        };

        let mut datagram = Vec::from(header.encode().unwrap());
        datagram.extend_from_slice(&[1, 2, 3, 4]);

        let (decoded, payload) = MediaDatagramHeader::decode(&datagram).unwrap();
        assert_eq!(decoded, header);
        assert_eq!(payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn packetizer_and_reassembler_handle_multiple_datagrams() {
        let mut packetizer = H264DatagramPacketizer::default();
        let data = vec![0x11; 4_200];
        let datagrams = packetizer
            .packetize_bytes(&data, true, 900, 300, 1_100)
            .unwrap();
        assert!(datagrams.len() > 1);

        let mut reassembler = H264DatagramReassembler::new(ReassemblerConfig::default());
        let now = Instant::now();
        let mut completed = None;
        for datagram in datagrams {
            completed = reassembler.push_datagram(&datagram, now).unwrap().or(completed);
        }

        let completed = completed.expect("expected completed access unit");
        assert_eq!(completed.data, data);
        assert!(completed.is_keyframe);
        assert_eq!(completed.pts_100ns, 900);
        assert_eq!(completed.duration_100ns, 300);
    }

    #[test]
    fn out_of_order_fragments_still_reassemble() {
        let mut packetizer = H264DatagramPacketizer::default();
        let data = (0..3_500u32).map(|value| (value % 251) as u8).collect::<Vec<_>>();
        let mut datagrams = packetizer
            .packetize_bytes(&data, false, 10, 10, 700)
            .unwrap();
        datagrams.reverse();

        let mut reassembler = H264DatagramReassembler::new(ReassemblerConfig::default());
        let mut completed = None;
        for datagram in datagrams {
            completed = reassembler
                .push_datagram(&datagram, Instant::now())
                .unwrap()
                .or(completed);
        }

        assert_eq!(completed.unwrap().data, data);
    }

    #[test]
    fn expired_incomplete_access_units_are_dropped() {
        let mut packetizer = H264DatagramPacketizer::default();
        let data = vec![0x22; 3_000];
        let datagrams = packetizer
            .packetize_bytes(&data, false, 1, 1, 900)
            .unwrap();

        let start = Instant::now();
        let mut reassembler = H264DatagramReassembler::new(ReassemblerConfig {
            incomplete_timeout: Duration::from_millis(5),
            max_in_flight_access_units: 4,
        });
        reassembler.push_datagram(&datagrams[0], start).unwrap();
        reassembler.prune_expired(start + Duration::from_millis(10));

        assert_eq!(reassembler.stats().dropped_incomplete_access_units, 1);
    }

    #[test]
    fn negotiated_payload_limit_returns_none_when_peer_disables_datagrams() {
        assert_eq!(negotiated_datagram_payload_limit(None, 1_100), None);
        assert_eq!(
            negotiated_datagram_payload_limit(Some(MEDIA_DATAGRAM_HEADER_LEN), 1_100),
            None
        );
    }

    #[test]
    fn invalid_fragment_shape_is_rejected() {
        let bytes = vec![0u8; MEDIA_DATAGRAM_HEADER_LEN];
        let error = MediaDatagramHeader::decode(&bytes).unwrap_err();
        assert!(matches!(
            error,
            MediaDatagramError::UnsupportedVersion { actual: 0 }
        ));
    }
}
