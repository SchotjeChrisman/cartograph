pub fn helper() -> usize {
    nested() + 1
}

fn nested() -> usize {
    fn inner() -> usize {
        0
    }
    inner()
}

pub mod deep {
    pub fn leaf() -> usize {
        super::helper()
    }
}
