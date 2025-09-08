// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//#![deny(warnings)]

#[cfg(test)]
mod tests {
    use midnight_serialize::*;
    use std::collections::HashMap;
    use std::fmt::Debug;
    use std::io::Write;
    use std::sync::Arc;

    #[derive(PartialEq, Debug, Clone)]
    struct SerializableStruct {
        foo: u64,
        bar: u64,
    }

    impl Serializable for SerializableStruct {
        fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
            self.foo.serialize(writer)?;
            self.bar.serialize(writer)?;
            Ok(())
        }
        fn serialized_size(&self) -> usize {
            self.foo.serialized_size() + self.bar.serialized_size()
        }
    }

    impl Deserializable for SerializableStruct {
        fn deserialize(
            reader: &mut impl std::io::Read,
            recursion_depth: u32,
        ) -> std::io::Result<Self> {
            Ok(Self {
                foo: u64::deserialize(reader, recursion_depth)?,
                bar: u64::deserialize(reader, recursion_depth)?,
            })
        }
    }

    #[derive(PartialEq, Clone, Debug)]
    struct TaggedStruct {
        foo: u64,
        bar: u64,
    }

    impl Tagged for TaggedStruct {
        fn tag() -> std::borrow::Cow<'static, str> {
            std::borrow::Cow::Borrowed("tagged-struct")
        }
        fn tag_unique_factor() -> String {
            "(u64,u64)".into()
        }
    }

    impl Serializable for TaggedStruct {
        fn serialized_size(&self) -> usize {
            self.foo.serialized_size() + self.bar.serialized_size()
        }

        fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
            self.foo.serialize(writer)?;
            self.bar.serialize(writer)?;
            Ok(())
        }
    }

    impl Deserializable for TaggedStruct {
        fn deserialize(
            reader: &mut impl std::io::Read,
            recursion_depth: u32,
        ) -> Result<Self, std::io::Error> {
            Ok(Self {
                foo: u64::deserialize(reader, recursion_depth)?,
                bar: u64::deserialize(reader, recursion_depth)?,
            })
        }
    }

    #[derive(PartialEq, Debug, Serializable)]
    #[tag = "derived-serializable"]
    struct DerivedSerializable {
        serializable: u8,
        tagged: TaggedStruct,
    }

    #[derive(PartialEq, Debug, Serializable)]
    #[tag = "derived-enum"]
    enum DerivedEnum {
        Unit,
        Tuple(u8, u8),
        Struct { foo: u8, bar: u8 },
    }

    #[test]
    fn serialize_serializable() {
        let mut writer = Vec::new();
        let value = SerializableStruct { foo: 5, bar: 10 };
        Serializable::serialize(&value, &mut writer).unwrap();
        assert_eq!(
            value,
            Deserializable::deserialize(&mut writer.as_slice(), 0).unwrap()
        );
    }

    #[test]
    fn serialize_tagged() {
        let mut writer = Vec::new();
        let value = TaggedStruct { foo: 2, bar: 5 };
        tagged_serialize(&value, &mut writer).unwrap();
        std::dbg!(&writer);

        assert!(writer.starts_with(b"midnight:tagged-struct:"));
        assert_eq!(value, tagged_deserialize(&mut writer.as_slice()).unwrap());
    }

    #[test]
    fn serialize_vec_serializable() {
        let mut writer = Vec::new();
        let value: Vec<SerializableStruct> = vec![SerializableStruct { foo: 4, bar: 10 }; 3];
        Serializable::serialize(&value, &mut writer).unwrap();
        assert_eq!(writer[0], 3 << 2); // Size
        let result: Vec<SerializableStruct> =
            Deserializable::deserialize(&mut writer.as_slice(), 0).unwrap();
        assert_eq!(value, result);
    }

    #[test]
    fn serialize_scale_ints() {
        fn ser(inp: impl Serializable + Deserializable + Eq + Debug) -> Vec<u8> {
            let mut writer = Vec::new();
            inp.serialize(&mut writer).unwrap();
            assert_eq!(
                inp,
                Deserializable::deserialize(&mut &writer[..], 0).unwrap()
            );
            writer
        }
        assert_eq!(ser(0u128), vec![0x00]);
        assert_eq!(ser(1u128), vec![0x04]);
        assert_eq!(ser(42u128), vec![0xa8]);
        assert_eq!(ser(69u128), vec![0x15, 0x01]);
        assert_eq!(ser(65535u128), vec![0xfe, 0xff, 0x03, 0x00]);
        assert_eq!(
            ser(100000000000000u128),
            vec![0x0b, 0x00, 0x40, 0x7a, 0x10, 0xf3, 0x5a]
        );
    }

    #[test]
    fn serialize_vec_scale_example() {
        let mut writer = Vec::new();
        let value = vec![4u16, 8u16, 15u16, 16u16, 23u16, 42u16];
        value.serialize(&mut writer).unwrap();
        assert_eq!(
            writer,
            vec![
                0x18, 0x04, 0x00, 0x08, 0x00, 0x0f, 0x00, 0x10, 0x00, 0x17, 0x00, 0x2a, 0x00
            ]
        );
        assert_eq!(value, <Vec<u16>>::deserialize(&mut &writer[..], 0).unwrap());
    }

    #[test]
    fn serialize_vec_tagged() {
        let mut writer = Vec::new();
        let value: Vec<TaggedStruct> = vec![TaggedStruct { foo: 4, bar: 10 }; 3];
        tagged_serialize(&value, &mut writer).unwrap();
        assert!(writer.starts_with(b"midnight:vec(tagged-struct):"));
        let result: Vec<TaggedStruct> = tagged_deserialize(&mut writer.as_slice()).unwrap();
        assert_eq!(value, result);
    }

    #[test]
    fn serialize_hashmap_serializable() {
        let mut writer = Vec::new();
        let mut value: HashMap<String, SerializableStruct> = HashMap::new();
        value.insert(String::from("test"), SerializableStruct { foo: 4, bar: 10 });
        value.insert(
            String::from("test1"),
            SerializableStruct { foo: 5, bar: 11 },
        );
        value.insert(
            String::from("test2"),
            SerializableStruct { foo: 6, bar: 12 },
        );
        Serializable::serialize(&value, &mut writer).unwrap();
        assert_eq!(writer[0], 3 << 2); // Size
        let result: HashMap<String, SerializableStruct> =
            Deserializable::deserialize(&mut writer.as_slice(), 0).unwrap();
        assert_eq!(value, result);
    }

    #[test]
    fn serialize_tuple() {
        let mut writer = Vec::new();
        let value: (SerializableStruct, TaggedStruct) = (
            SerializableStruct { foo: 5, bar: 11 },
            TaggedStruct { foo: 4, bar: 10 },
        );
        Serializable::serialize(&value, &mut writer).unwrap();
        let result = Deserializable::deserialize(&mut writer.as_slice(), 0).unwrap();
        assert_eq!(value, result);
    }

    #[test]
    fn serialize_ref_tuple() {
        let mut writer = Vec::new();
        let a = SerializableStruct { foo: 5, bar: 11 };
        let b = TaggedStruct { foo: 4, bar: 10 };
        let value = (&a, &b);
        Serializable::serialize(&value, &mut writer).unwrap();
        let result: (SerializableStruct, TaggedStruct) =
            Deserializable::deserialize(&mut writer.as_slice(), 0).unwrap();
        assert_eq!(*value.0, result.0);
        assert_eq!(*value.1, result.1);
    }

    #[test]
    fn error_on_too_few_bytes() {
        let mut writer = Vec::new();
        let value = TaggedStruct { foo: 5, bar: 11 };
        tagged_serialize(&value, &mut writer).unwrap();
        writer.pop();
        assert!(tagged_deserialize::<TaggedStruct>(writer.as_slice()).is_err());
    }

    #[test]
    fn error_on_left_over_bytes() {
        let mut writer = Vec::new();
        let value = TaggedStruct { foo: 5, bar: 11 };
        writer.push(0);
        tagged_serialize(&value, &mut writer).unwrap();
        assert!(tagged_deserialize::<TaggedStruct>(writer.as_slice()).is_err());
    }

    #[test]
    fn derive_serializable() {
        let mut writer = Vec::new();
        let value = DerivedSerializable {
            serializable: 4,
            tagged: TaggedStruct { foo: 6, bar: 7 },
        };
        tagged_serialize(&value, &mut writer).unwrap();
        assert_eq!(value, tagged_deserialize(&mut writer.as_slice()).unwrap());
    }

    #[test]
    fn option_serializable() {
        let mut writer = Vec::new();

        let some_val: Option<TaggedStruct> = Some(TaggedStruct { foo: 2, bar: 3 });
        let none_val: Option<TaggedStruct> = None;

        tagged_serialize(&some_val, &mut writer).unwrap();
        assert_eq!(
            some_val,
            tagged_deserialize(&mut writer.as_slice()).unwrap()
        );

        writer = Vec::new();
        tagged_serialize(&none_val, &mut writer).unwrap();
        assert_eq!(
            none_val,
            tagged_deserialize(&mut writer.as_slice()).unwrap()
        );
    }

    #[test]
    fn serialized_size() {
        let ss = SerializableStruct { foo: 1, bar: 2 };
        let vs = TaggedStruct { foo: 3, bar: 4 };
        let ds = DerivedSerializable {
            serializable: 4,
            tagged: vs.clone(),
        };
        assert_eq!(2, SerializableStruct::serialized_size(&ss));
        assert_eq!(2, TaggedStruct::serialized_size(&vs));
        assert_eq!(3, DerivedSerializable::serialized_size(&ds));
    }

    #[test]
    fn enum_derive() {
        let unit = DerivedEnum::Unit;
        let tuple = DerivedEnum::Tuple(2, 3);
        let strct = DerivedEnum::Struct { foo: 1, bar: 4 };

        let mut writer = Vec::new();
        tagged_serialize(&unit, &mut writer).unwrap();
        assert_eq!(unit, tagged_deserialize(&mut writer.as_slice()).unwrap());

        writer = Vec::new();
        tagged_serialize(&tuple, &mut writer).unwrap();
        assert_eq!(tuple, tagged_deserialize(&mut writer.as_slice()).unwrap());

        writer = Vec::new();
        tagged_serialize(&strct, &mut writer).unwrap();
        assert_eq!(strct, tagged_deserialize(&mut writer.as_slice()).unwrap());
    }

    #[test]
    fn recursive_limit() {
        enum Recursive {
            None,
            Some(Arc<Recursive>),
        }

        impl Serializable for Recursive {
            fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
                match self {
                    Recursive::None => <u8 as Serializable>::serialize(&0, writer),
                    Recursive::Some(child) => {
                        <u8 as Serializable>::serialize(&1, writer)?;
                        Recursive::serialize(child, writer)
                    }
                }
            }

            fn serialized_size(&self) -> usize {
                match self {
                    Recursive::None => 1,
                    Recursive::Some(child) => 1 + Recursive::serialized_size(child),
                }
            }
        }

        impl Deserializable for Recursive {
            fn deserialize(
                reader: &mut impl std::io::Read,
                mut recursion_depth: u32,
            ) -> Result<Self, std::io::Error> {
                Self::check_rec(&mut recursion_depth)?;
                let det = <u8 as Deserializable>::deserialize(reader, recursion_depth)?;
                match det {
                    0 => Ok(Recursive::None),
                    1 => Ok(Recursive::Some(Arc::new(Recursive::deserialize(
                        reader,
                        recursion_depth,
                    )?))),
                    _ => unreachable!(),
                }
            }
        }

        let mut value = Recursive::None;
        for _ in 0..(RECURSION_LIMIT + 1) {
            value = Recursive::Some(Arc::new(value));
        }

        let mut bytes: Vec<u8> = Vec::new();
        value.serialize(&mut bytes).unwrap();
        let res: Result<Recursive, std::io::Error> =
            Deserializable::deserialize(&mut bytes.as_slice(), 0);
        assert!(res.is_err())
    }
}
