pub trait Device {
    fn poll(&mut self) -> Vec<crate::InputEvent>;
    fn name(&self) -> &str;
    fn id(&self) -> &str;
}
