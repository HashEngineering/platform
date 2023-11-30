impl :: bincode :: Encode for IdentifierBytes32
{
    fn encode < __E : :: bincode :: enc :: Encoder >
    (& self, encoder : & mut __E) -> core :: result :: Result < (), :: bincode
    :: error :: EncodeError >
    { :: bincode :: Encode :: encode(& self.0, encoder) ? ; Ok(()) }
}