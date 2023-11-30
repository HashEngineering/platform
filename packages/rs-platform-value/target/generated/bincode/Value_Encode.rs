impl :: bincode :: Encode for Value
{
    fn encode < __E : :: bincode :: enc :: Encoder >
    (& self, encoder : & mut __E) -> core :: result :: Result < (), :: bincode
    :: error :: EncodeError >
    {
        match self
        {
            Self :: U128(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (0u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: I128(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (1u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: U64(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (2u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: I64(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (3u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: U32(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (4u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: I32(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (5u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: U16(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (6u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: I16(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (7u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: U8(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (8u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: I8(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (9u32), encoder) ?
                ; :: bincode :: Encode :: encode(field_0, encoder) ? ; Ok(())
            }, Self :: Bytes(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (10u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Bytes20(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (11u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Bytes32(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (12u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Bytes36(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (13u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: EnumU8(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (14u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: EnumString(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (15u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Identifier(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (16u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Float(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (17u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Text(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (18u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Bool(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (19u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Null =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (20u32), encoder)
                ? ; Ok(())
            }, Self :: Array(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (21u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            }, Self :: Map(field_0) =>
            {
                < u32 as :: bincode :: Encode > :: encode(& (22u32), encoder)
                ? ; :: bincode :: Encode :: encode(field_0, encoder) ? ;
                Ok(())
            },
        }
    }
}