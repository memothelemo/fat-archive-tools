macro_rules! assert_io_err {
    ($expr:expr, $kind:ident) => {
        match $expr {
            Ok(..) => panic!("unexpected Ok(..)"),
            Err(error) => assert_eq!(
                error.kind(),
                io::ErrorKind::$kind,
                "unexpected given different error ({:?}): {}",
                error.kind(),
                error
            ),
        }
    };
}
pub(crate) use assert_io_err;
