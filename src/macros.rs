macro_rules! define_tag {
    (
        #[repr($reprty:ty)]
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$membermeta:meta])*
                $membername:ident = $membervalue:literal
            ),*

            $(,)?
        }
    ) => {
        #[repr($reprty)]
        $(#[$meta])*
        $vis enum $name {
            $(
                $(#[$membermeta])*
                $membername = $membervalue,
            )*
        }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$membername),*];
        }

        impl From<$name> for $reprty {
            fn from(value: $name) -> $reprty {
                match value {
                    $(
                        $name::$membername => $membervalue,
                    )*
                }
            }
        }

        impl TryFrom<$reprty> for $name {
            type Error = $reprty;

            fn try_from(value: $reprty) -> Result<$name, $reprty> {
                match value {
                    $($membervalue => Ok($name::$membername),)*
                    invalid => Err(invalid),
                }
            }
        }
    };
}
