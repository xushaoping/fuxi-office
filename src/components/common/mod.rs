//! Small, reusable UI primitives used by the workspace.

mod a11y;
mod button;
mod input;
mod lazy_image;
mod loading;
mod modal;
mod page_transition;
mod toast;
mod virtual_list;

pub use a11y::{LivePoliteness, LiveRegion};
pub use button::{Button, ButtonSize, ButtonVariant};
pub use input::AccessibleInput;
pub use lazy_image::LazyImage;
pub use loading::{LoadingSize, LoadingSpinner, Skeleton, SkeletonVariant};
pub use modal::Modal;
pub use page_transition::PageTransition;
pub use toast::ToastContainer;
pub use virtual_list::VirtualList;
