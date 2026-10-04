//! 对话框：添加/编辑仓库、初始化向导、分叉恢复、克隆、命令编辑、提交并推送、设置、批量结果。
//!
//! 各自独立成文件的原因是它们共用同一套 Dialog 外壳（`dlg_footer` 的按钮排布、
//! Esc 关闭、焦点归位），但内容互不相干。拆开是为了改一个对话框时 diff 里
//! 不会带出另外两个。

pub(crate) mod batch_log;
pub(crate) mod clean;
pub(crate) mod clone;
pub(crate) mod cmds;
pub(crate) mod commit;
pub(crate) mod fork;
pub(crate) mod init;
pub(crate) mod repo;
pub(crate) mod settings;
pub(crate) mod sync;
