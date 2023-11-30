impl :: bincode :: Decode for Value
{
    fn decode < __D : :: bincode :: de :: Decoder > (decoder : & mut __D) ->
    core :: result :: Result < Self, :: bincode :: error :: DecodeError >
    {
        let variant_index = < u32 as :: bincode :: Decode > :: decode(decoder)
        ? ; match variant_index
        {
            0u32 =>
            Ok(Self :: U128
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 1u32 =>
            Ok(Self :: I128
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 2u32 =>
            Ok(Self :: U64
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 3u32 =>
            Ok(Self :: I64
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 4u32 =>
            Ok(Self :: U32
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 5u32 =>
            Ok(Self :: I32
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 6u32 =>
            Ok(Self :: U16
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 7u32 =>
            Ok(Self :: I16
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 8u32 =>
            Ok(Self :: U8 { 0 : :: bincode :: Decode :: decode(decoder) ?, }),
            9u32 =>
            Ok(Self :: I8 { 0 : :: bincode :: Decode :: decode(decoder) ?, }),
            10u32 =>
            Ok(Self :: Bytes
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 11u32 =>
            Ok(Self :: Bytes20
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 12u32 =>
            Ok(Self :: Bytes32
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 13u32 =>
            Ok(Self :: Bytes36
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 14u32 =>
            Ok(Self :: EnumU8
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 15u32 =>
            Ok(Self :: EnumString
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 16u32 =>
            Ok(Self :: Identifier
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 17u32 =>
            Ok(Self :: Float
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 18u32 =>
            Ok(Self :: Text
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 19u32 =>
            Ok(Self :: Bool
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 20u32 =>
            Ok(Self :: Null {}), 21u32 =>
            Ok(Self :: Array
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), 22u32 =>
            Ok(Self :: Map
            { 0 : :: bincode :: Decode :: decode(decoder) ?, }), variant =>
            Err(:: bincode :: error :: DecodeError :: UnexpectedVariant
            {
                found : variant, type_name : "Value", allowed : & :: bincode
                :: error :: AllowedEnumVariants :: Range { min : 0, max : 22 }
            })
        }
    }
} impl < '__de > :: bincode :: BorrowDecode < '__de > for Value
{
    fn borrow_decode < __D : :: bincode :: de :: BorrowDecoder < '__de > >
    (decoder : & mut __D) -> core :: result :: Result < Self, :: bincode ::
    error :: DecodeError >
    {
        let variant_index = < u32 as :: bincode :: Decode > :: decode(decoder)
        ? ; match variant_index
        {
            0u32 =>
            Ok(Self :: U128
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            1u32 =>
            Ok(Self :: I128
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            2u32 =>
            Ok(Self :: U64
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            3u32 =>
            Ok(Self :: I64
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            4u32 =>
            Ok(Self :: U32
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            5u32 =>
            Ok(Self :: I32
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            6u32 =>
            Ok(Self :: U16
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            7u32 =>
            Ok(Self :: I16
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            8u32 =>
            Ok(Self :: U8
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            9u32 =>
            Ok(Self :: I8
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            10u32 =>
            Ok(Self :: Bytes
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            11u32 =>
            Ok(Self :: Bytes20
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            12u32 =>
            Ok(Self :: Bytes32
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            13u32 =>
            Ok(Self :: Bytes36
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            14u32 =>
            Ok(Self :: EnumU8
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            15u32 =>
            Ok(Self :: EnumString
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            16u32 =>
            Ok(Self :: Identifier
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            17u32 =>
            Ok(Self :: Float
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            18u32 =>
            Ok(Self :: Text
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            19u32 =>
            Ok(Self :: Bool
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            20u32 => Ok(Self :: Null {}), 21u32 =>
            Ok(Self :: Array
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            22u32 =>
            Ok(Self :: Map
            { 0 : :: bincode :: BorrowDecode :: borrow_decode(decoder) ?, }),
            variant =>
            Err(:: bincode :: error :: DecodeError :: UnexpectedVariant
            {
                found : variant, type_name : "Value", allowed : & :: bincode
                :: error :: AllowedEnumVariants :: Range { min : 0, max : 22 }
            })
        }
    }
}