/// Define a two-variant enum that replaces a bare `bool` parameter.
///
/// Generates `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`, `From<bool>`
/// (first variant = `false`, second = `true`).
///
/// ```ignore
/// bool_enum! {
///     /// Whether a config operation targets global or local scope.
///     pub ConfigScope { Local, Global }
/// }
/// // ConfigScope::from(true) == Global
/// // ConfigScope::from(false) == Local
/// ```
macro_rules! bool_enum {
    (
        $(#[$meta:meta])*
        $vis:vis $Name:ident {
            $(#[$false_meta:meta])*
            $False:ident,
            $(#[$true_meta:meta])*
            $True:ident $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $Name {
            $(#[$false_meta])*
            $False,
            $(#[$true_meta])*
            $True,
        }

        impl From<bool> for $Name {
            fn from(value: bool) -> Self {
                if value { Self::$True } else { Self::$False }
            }
        }
    };
}

pub(crate) use bool_enum;
