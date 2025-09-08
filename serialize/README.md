This crate defines the `Serializable`, `Deserializable`, and `Versioned` traits, and the
top-level serialization functions `serialize` and `deserialize` which require
`Serializable` and `Deserializable` to be implemented on their respective
arguments.

`Serializable` objects are either versioned or not. This is defined by a
`const Option<Version>` in the trait implementation for `Versioned`.

All calls to serialize and deserialize objects should be done with
`serialize::serialize()` and `serialize::deserialize()` respectively, as they
include checks against unused bytes in the reader object. Similarly all
deserialization implementations should not error when there are bytes remaining
as objects are often recursively deserialized.

In practice there are four classes of objects that will implement these traits:
* Top level versioned objects. These will include version information and
    deserialization logic that branches on version information. If the object
    has child elements  implementing `Serializable`/`Deserializable` the type
    should overload `Serializable::serialize` to ensure the proper
    serialization methods are called on any child elements
    ```rust
    impl Versioned for Type {
        const VERSION: Option<serialize::Version> = Some(Version {major: 1, minor: 0});
    }

    impl Serializable for Type {}
 
    impl<T> Deserializable for Complex<T> {
       fn versioned_deserialize<R: std::io::Read>(
               reader: &mut R,
               version: &Option<serialize::Version>,
            ) -> Result<Self, std::io::Error> {
            match version {
                Some(Version {major: 1, minor: 0}) => {
                    // deserialization logic
                }
                _ => Err(Self::deserialization_error(version))
            }
        }
    }
    ```
* Unversioned objects (`String`, `u64`, etc.). These can use the
    `unversioned_serializeable!(type)` macro to build out the boilerplate
    implementation for simple types. For complex types the boilerplate may need
    to be written by hand. Care should be taken to ensure any child elements
    implementing `Serializable`/`Deserializable` call the correct serialization
    functions, this will require overloading `Serializable::serialize`.
    ```rust
    impl<T> Versioned for Complex<T> {
        const VERSION: Option<serialize::Version> = None;
    }
 
    impl<T> Serializable for Complex<T> {}

    impl<T> Deserializable for Complex<T> {
       fn versioned_deserialize<R: std::io::Read>(
               reader: &mut R,
               _version: &Option<serialize::Version>,   // will be None
           ) -> Result<Self, std::io::Error> {
            //deserialization logic
       }
    }
    ```
* Standard containers containing `Serializable`/`Deserializable` types. These
    have been implemented for `Vec`, `HashMap` and `Option`.
* Complex unversioned objects entirely containing `Serializable`/`Deserializable` objects.
    The child elements can be versioned or unversioned.
    The `Serializable` and `Deserializable` traits can be derived via a macro.
    ```rust
    #[derive(Serializable, Deserializable)]
    struct ComplexUnversioned {
        versioned: VersionedStruct,
        unversioned: UnversionedStruct
    }
    ```

`Serializable::serialize` should not be overloaded, and instead one should
`Serializable::serialize_inner` with custom serialization logic if required
```rust
impl Versioned for Type {
    const Version = Some(Version { major: 1, minor: 0});
}

impl Serializable for Type {
    fn serialize_inner<W: Write>(value: &Self, writer: &mut W) -> Result<(), std::io::Error> {
        // serialization logic
    }
}
```

# Running tests
```
NETWORK_ID=1 cargo test --release
```
