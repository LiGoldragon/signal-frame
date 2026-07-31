/// Request-payload enums expose their DOTOS record heads through this
/// trait. `signal_channel!` implements it for the generated operation
/// enum so command-line dispatch can route before full decode.
pub trait SignalOperationHeads {
    const HEADS: &'static [&'static str];

    fn contains_head(head: &str) -> bool {
        Self::HEADS.contains(&head)
    }
}
