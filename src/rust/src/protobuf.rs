//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

#![allow(clippy::derive_partial_eq_without_eq)]

pub mod group_call {
    call_protobuf::include_groupcall_proto!();

    impl SfuToDevice {
        fn is_extendable(&self) -> bool {
            self.mrp_header
                .map(|h| h.num_packets.is_some())
                .unwrap_or(false)
        }
    }

    impl Extend<SfuToDevice> for SfuToDevice {
        fn extend<T: IntoIterator<Item = SfuToDevice>>(&mut self, iter: T) {
            if self.is_extendable() {
                if self.content.is_none() {
                    self.content = Some(Vec::new());
                }
                let content = self.content.as_mut().unwrap();
                for message in iter {
                    if let Some(other_content) = message.content {
                        content.extend(other_content);
                    }
                }
            }
        }
    }

    impl DeviceToSfu {
        fn is_extendable(&self) -> bool {
            self.mrp_header
                .map(|h| h.num_packets.is_some())
                .unwrap_or(false)
        }
    }

    impl Extend<DeviceToSfu> for DeviceToSfu {
        fn extend<T: IntoIterator<Item = DeviceToSfu>>(&mut self, iter: T) {
            if self.is_extendable() {
                if self.content.is_none() {
                    self.content = Some(Vec::new());
                }
                let content = self.content.as_mut().unwrap();
                for message in iter {
                    if let Some(other_content) = message.content {
                        content.extend(other_content);
                    }
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn gen_bytes(i: usize) -> Vec<u8> {
            format!("{i}").repeat(i).into_bytes()
        }

        fn sfu_to_device_content_messages(i: usize) -> Vec<SfuToDevice> {
            (2..(i + 2))
                .map(|i| SfuToDevice {
                    mrp_header: Some(MrpHeader {
                        seqnum: Some(i as u64),
                        num_packets: if i == 1 { Some(10) } else { None },
                        ..Default::default()
                    }),
                    content: Some(gen_bytes(i)),
                    ..Default::default()
                })
                .collect::<Vec<SfuToDevice>>()
        }

        #[test]
        fn test_sfu_to_device_extendable() {
            fn empty_content() -> SfuToDevice {
                SfuToDevice {
                    mrp_header: Some(MrpHeader {
                        seqnum: Some(1),
                        num_packets: Some(10),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            }

            let mut first = empty_content();
            let messages = sfu_to_device_content_messages(10);

            let mut expected = vec![];
            for (i, message) in messages.iter().enumerate() {
                expected.extend(gen_bytes(i + 2));
                first.extend(vec![message.clone()]);
                assert_eq!(first.content.as_ref().unwrap(), &expected);
            }

            let mut first = empty_content();
            first.extend(messages);
            assert_eq!(first.content.as_ref().unwrap(), &expected);
        }

        #[test]
        fn test_sfu_to_device_not_extendable() {
            let messages = sfu_to_device_content_messages(10);
            let mut no_header = SfuToDevice {
                mrp_header: None,
                ..Default::default()
            };
            no_header.extend(messages.clone());
            assert_eq!(no_header.content, None);

            let mut no_num_packets = SfuToDevice {
                mrp_header: Some(MrpHeader {
                    num_packets: None,
                    ..Default::default()
                }),
                ..Default::default()
            };
            no_num_packets.extend(messages);
            assert_eq!(no_num_packets.content, None);
        }

        fn device_to_sfu_content_messages(i: usize) -> Vec<DeviceToSfu> {
            (2..(i + 2))
                .map(|i| DeviceToSfu {
                    mrp_header: Some(MrpHeader {
                        seqnum: Some(i as u64),
                        num_packets: if i == 1 { Some(10) } else { None },
                        ..Default::default()
                    }),
                    content: Some(gen_bytes(i)),
                    ..Default::default()
                })
                .collect::<Vec<DeviceToSfu>>()
        }

        #[test]
        fn test_device_to_sfu_extendable() {
            fn empty_content() -> DeviceToSfu {
                DeviceToSfu {
                    mrp_header: Some(MrpHeader {
                        seqnum: Some(1),
                        num_packets: Some(10),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            }

            let mut first = empty_content();
            let messages = device_to_sfu_content_messages(10);

            let mut expected = vec![];
            for (i, message) in messages.iter().enumerate() {
                expected.extend(gen_bytes(i + 2));
                first.extend(vec![message.clone()]);
                assert_eq!(first.content.as_ref().unwrap(), &expected);
            }

            let mut first = empty_content();
            first.extend(messages);
            assert_eq!(first.content.as_ref().unwrap(), &expected);
        }

        #[test]
        fn test_device_to_sfu_not_extendable() {
            let messages = device_to_sfu_content_messages(10);
            let mut no_header = DeviceToSfu {
                mrp_header: None,
                ..Default::default()
            };
            no_header.extend(messages.clone());
            assert_eq!(no_header.content, None);

            let mut no_num_packets = DeviceToSfu {
                mrp_header: Some(MrpHeader {
                    num_packets: None,
                    ..Default::default()
                }),
                ..Default::default()
            };
            no_num_packets.extend(messages);
            assert_eq!(no_num_packets.content, None);
        }
    }
}

pub mod rtp_data {
    call_protobuf::include_rtp_proto!();
}

pub mod signaling {
    call_protobuf::include_signaling_proto!();
}
