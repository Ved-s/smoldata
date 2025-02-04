macro_rules! define_tag {
    (@unpackarm $unpackty:ident exact $($unpack:tt)*) => {
        $unpackty::$($unpack)*
    };

    (@unpackarm $unpackty:ident pack ($($_:tt)*) unpack ($($unpack:tt)*)) => {
        $unpackty::$($unpack)*
    };

    (@packarm $unpackty:ident exact $($pack:tt)*) => {
        $unpackty::$($pack)*
    };

    (@packarm $unpackty:ident pack ($($pack:tt)*) unpack ($($_:tt)*)) => {
        $unpackty::$($pack)*
    };

    (
        #[repr($reprty:ty)]
        #[unpack($unpackty:ident)]
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                #[unpack($($unpacktt:tt)*)]
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

            pub const fn unpack(self) -> $unpackty {
                match self {
                    $(
                        Self::$membername => define_tag!(@unpackarm $unpackty $($unpacktt)*),
                    )*
                }
            }
        }

        impl $unpackty {
            pub const fn pack(self) -> $name {
                match self {
                    $(
                        define_tag!(@packarm $unpackty $($unpacktt)*) => $name::$membername,
                    )*
                }
            }
        }

        impl From<$name> for $unpackty {
            fn from(value: $name) -> $unpackty {
                value.unpack()
            }
        }

        impl From<$unpackty> for $name {
            fn from(value: $unpackty) -> $name {
               value.pack()
            }
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
