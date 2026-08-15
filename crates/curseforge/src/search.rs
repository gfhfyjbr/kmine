use crate::ClassId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryFilter {
    All,
    ClassesOnly,
    ChildrenOf(ClassId),
}
