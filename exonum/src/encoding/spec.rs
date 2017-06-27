/// `encoding_struct!` implement structure that could be saved in blockchain.
///
/// Storage value unlike message, could be mapped on buffers without any checks.
/// For now it's required to set `encoding_struct` fixed part size as `const SIZE`.
///
/// For each field, it's required to set exact position in `encoding_struct`.
/// # Usage Example:
/// ```
/// #[macro_use] extern crate exonum;
/// # extern crate serde;
/// # extern crate serde_json;
///
/// encoding_struct! {
///     struct SaveTwoInteger {
///         const SIZE = 16;
///         
///         field first: u64 [0 => 8]
///         field second: u64 [8 => 16]
///     }
/// }
/// # fn main() {
///     let first = 1u64;
///     let second = 2u64;
///     let s = SaveTwoInteger::new(first, second);
///     println!("Debug structure = {:?}", s);
/// # }
/// ```
///
/// For additional reference about data layout see also 
/// *[ `encoding` documentation](./encoding/index.html).*
///
/// `encoding_struct!` internaly use `ident_count!`, be sure to add this macro to namespace.
#[macro_export]
macro_rules! encoding_struct {
    (
    $(#[$attr:meta])*
    struct $name:ident {
        const SIZE = $body:expr;

        $(
        $(#[$field_attr:meta])*
        field $field_name:ident : $field_type:ty [$from:expr => $to:expr]
        )*
    }) => (
        #[derive(Clone, PartialEq)]
        $(#[$attr])*
        pub struct $name {
            raw: Vec<u8>
        }

        // Reimplement `Field` for `encoding_struct!` 
        // to write fields in place of another structure
        impl<'a> $crate::encoding::Field<'a> for $name {
            unsafe fn read(buffer: &'a [u8],
                            from: $crate::encoding::Offset,
                            to: $crate::encoding::Offset) -> Self {
                let vec: Vec<u8> = $crate::encoding::Field::read(buffer, from, to);
                $crate::storage::StorageValue::deserialize(vec)
            }

            fn write(&self,
                            buffer: &mut Vec<u8>,
                            from: $crate::encoding::Offset,
                            to: $crate::encoding::Offset) {
                $crate::encoding::Field::write(&self.raw, buffer, from, to);
            }
            
            #[allow(unused_variables)]
            fn check(buffer: &'a [u8],
                        from_st_val: $crate::encoding::CheckedOffset,
                        to_st_val: $crate::encoding::CheckedOffset,
                        latest_segment: $crate::encoding::CheckedOffset)
                -> $crate::encoding::Result
            {
                let latest_segment_origin = <&[u8] as $crate::encoding::Field>::check(buffer, from_st_val, to_st_val, latest_segment)?;
                let vec: &[u8] = unsafe{ $crate::encoding::Field::read(buffer, 
                                                                        from_st_val.unchecked_offset(),
                                                                        to_st_val.unchecked_offset())};
                let latest_segment = ($body as $crate::encoding::Offset).into();
                $(
                let latest_segment = <$field_type as $crate::encoding::Field>::check(&vec,
                                                                        $from.into(),
                                                                        $to.into(),
                                                                        latest_segment)?;
                )*
                Ok(latest_segment_origin)
            }

            fn field_size() -> $crate::encoding::Offset {
                // We write `encoding_struct` as regular buffer,
                // so real `field_size` is 8.
                // TODO: maybe we should write it as sub structure in place?
                // We could get benefit from it: we limit indirection
                // in deserializing sub fields, by only one calculation.

                // $body as $crate::encoding::Offset

                8 as $crate::encoding::Offset
            }
        }

        impl $crate::storage::StorageValue for $name {
            fn serialize(self) -> Vec<u8> {
                self.raw
            }

            fn deserialize(v: Vec<u8>) -> Self {
                $name {
                    raw: v
                }
            }

            fn hash(&self) -> $crate::crypto::Hash {
                $name::hash(self)
            }
        }

        // TODO extract some fields like hash and from_raw into trait
        impl $name {
            #[cfg_attr(feature="cargo-clippy", allow(too_many_arguments))]
            /// Create `$name`.
            pub fn new($($field_name: $field_type,)*) -> $name {
                use $crate::encoding::{Field};
                let mut buf = vec![0; $body];
                $($field_name.write(&mut buf, $from, $to);)*
                $name { raw: buf }
            }

            /// Hashes the `$name` as a raw byte array and returns the resulting hash.
            pub fn hash(&self) -> $crate::crypto::Hash {
                $crate::crypto::hash(self.raw.as_ref())
            }

            $(
            $(#[$field_attr])*
            pub fn $field_name(&self) -> $field_type {
                use $crate::encoding::Field;
                unsafe {
                    Field::read(&self.raw, $from, $to)
                }
            })*
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, fmt: &mut ::std::fmt::Formatter)
                -> Result<(), ::std::fmt::Error> {
                fmt.debug_struct(stringify!($name))
                 $(.field(stringify!($field_name), &self.$field_name()))*
                   .finish()
            }
        }

        impl $crate::encoding::serialize::json::ExonumJson for $name {
            fn deserialize_field<B> (value: &$crate::encoding::serialize::json::reexport::Value,
                                        buffer: & mut B,
                                        from: $crate::encoding::Offset,
                                        _to: $crate::encoding::Offset )
                -> Result<(), Box<::std::error::Error>>
                where B: $crate::encoding::serialize::WriteBufferWrapper
            {
                use $crate::encoding::serialize::json::ExonumJson;
                let obj = value.as_object().ok_or("Can't cast json as object.")?;
                $(
                let val = obj.get(stringify!($field_name)).ok_or("Can't get object from json.")?;

                <$field_type as ExonumJson>::deserialize_field(val, buffer,
                                                                from + $from, from + $to )?;

                )*
                Ok(())
            }

            fn serialize_field(&self) 
                -> Result<$crate::encoding::serialize::json::reexport::Value,
                            Box<::std::error::Error>>
            {
                use $crate::encoding::serialize::json::reexport::Value;
                let mut map = $crate::encoding::serialize::json::reexport::Map::new();
                $(
                    map.insert(stringify!($field_name).to_string(),
                        self.$field_name().serialize_field()?);
                )*
                Ok(Value::Object(map))
            }
        }
        impl $crate::encoding::serialize::json::ExonumJsonDeserialize for $name {
            fn deserialize(value: &$crate::encoding::serialize::json::reexport::Value) -> Result<Self, Box<::std::error::Error>> {
                let to = $body as $crate::encoding::Offset;
                let from = 0;
                use $crate::encoding::serialize::json::ExonumJson;

                let mut buf = vec![0; $body];
                <Self as ExonumJson>::deserialize_field(value, &mut buf, from, to)?;
                Ok($name { raw: buf })
            }
        }

        // TODO: Rewrite Deserialize and Serialize implementation
        impl<'de> $crate::encoding::serialize::reexport::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where D: $crate::encoding::serialize::reexport::Deserializer<'de>
            {
                use $crate::encoding::serialize::json::reexport::Value;
                use $crate::encoding::serialize::reexport::{DeError, Deserialize};
                let value = <Value as Deserialize>::deserialize(deserializer)?;
                <Self as $crate::encoding::serialize::json::ExonumJsonDeserialize>::deserialize(&value)
                .map_err(|_| D::Error::custom("Can not deserialize value."))
            }
        }

        impl $crate::encoding::serialize::reexport::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where S: $crate::encoding::serialize::reexport::Serializer
            {
                use $crate::encoding::serialize::reexport::SerError;
                use $crate::encoding::serialize::json::ExonumJson;
                self.serialize_field()
                    .map_err(|_| S::Error::custom(
                                concat!("Can not serialize structure: ", stringify!($name))))?
                    .serialize(serializer)
            }
        }
    )
}
