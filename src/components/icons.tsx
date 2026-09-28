/** 统一的线性 SVG 图标（stroke 风格，颜色跟随 currentColor） */
export function Icon({
  size = 16,
  className,
  children,
  viewBox = "0 0 24 24",
  strokeWidth = 1.7,
}: {
  size?: number;
  className?: string;
  children: React.ReactNode;
  viewBox?: string;
  strokeWidth?: number;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox={viewBox}
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden
    >
      {children}
    </svg>
  );
}

/** 左箭头（侧栏收展把手） */
export const IconChevronLeft = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M15 18l-6-6 6-6" />
  </Icon>
);

/** 面板开关（侧栏收展：标题栏 logo 的 hover 态） */
export const IconPanelLeft = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M9 4v16" />
  </Icon>
);

/** 窗控：最小化 / 最大化 / 还原 */
export const IconMinus = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M5 12h14" />
  </Icon>
);

export const IconMaximize = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M16 21h3a2 2 0 0 0 2-2v-3M8 21H5a2 2 0 0 1-2-2v-3" />
  </Icon>
);

export const IconRestore = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <rect x="8" y="8" width="12" height="12" rx="2" />
    <path d="M4 16V6a2 2 0 0 1 2-2h10" />
  </Icon>
);

/** 仓库（书本形 repo 标） */
export const IconRepo = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
    <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
  </Icon>
);

export const IconPlus = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M12 5v14M5 12h14" />
  </Icon>
);

/** 新建仓库（文件 + 号） */
export const IconFilePlus = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6M12 18v-6M9 15h6" />
  </Icon>
);

/** 克隆（下载到托盘） */
export const IconDownload = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <path d="M7 10l5 5 5-5M12 15V3" />
  </Icon>
);

export const IconPencil = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z" />
  </Icon>
);

export const IconX = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M18 6L6 18M6 6l12 12" />
  </Icon>
);

export const IconFolder = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
  </Icon>
);

export const IconTerminal = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M4 17l6-6-6-6M12 19h8" />
  </Icon>
);

/** 提交历史（时钟） */
export const IconHistory = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 3" />
  </Icon>
);

/** 待提交变更（文件差异） */
export const IconDiff = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6M8 13h3M9.5 11.5v3M15 13h.01M15 17H9" />
  </Icon>
);

/** 普通文件 */
export const IconFile = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6" />
  </Icon>
);

export const IconGear = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </Icon>
);

export const IconMoon = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
  </Icon>
);

export const IconSun = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
  </Icon>
);

/** 跟随系统（显示器） */
export const IconMonitor = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <rect x="2" y="3" width="20" height="14" rx="2" />
    <path d="M8 21h8M12 17v4" />
  </Icon>
);

/** 扫描（同步箭头） */
export const IconSync = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M23 4v6h-6M1 20v-6h6" />
    <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
  </Icon>
);

/** 闹钟（定时自动同步）：表盘偏下 + 顶部两个铃铛斜线，与提交历史的纯时钟区分 */
export const IconClock = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <circle cx="12" cy="13" r="8" />
    <path d="M12 9.5V13l2.5 1.8" />
    <path d="M5 3 2.5 5.5M19 3l2.5 2.5" />
  </Icon>
);

/** 图钉（置顶仓库） */
export const IconPin = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M9 4h6v4.5l2.5 2.5v1.5h-11v-1.5L9 8.5V4z" />
    <path d="M12 12.5V20" />
  </Icon>
);

/** 搜索（放大镜） */
export const IconSearch = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <circle cx="11" cy="11" r="7" />
    <path d="m21 21-4.3-4.3" />
  </Icon>
);

/** 空心云朵 */
export const IconCloud = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z" />
  </Icon>
);

/** 拉取：云朵 + 向下箭头（空心） */
export const IconCloudDown = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M20.88 18.09A5 5 0 0 0 18 9h-1.26A8 8 0 1 0 3 16.29" />
    <path d="M16 14l-4 4-4-4M12 12v9" />
  </Icon>
);

/** 推送：云朵 + 向上箭头（空心） */
export const IconCloudUp = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M20.88 18.09A5 5 0 0 0 18 9h-1.26A8 8 0 1 0 3 16.29" />
    <path d="M16 19l-4-4-4 4M12 21v-9" />
  </Icon>
);

/** 磁盘（空间扫描） */
export const IconDisk = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M22 12H2" />
    <path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" />
    <path d="M6 16h.01M10 16h.01" />
  </Icon>
);

/** 播放（跑测试） */
export const IconPlay = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M7 4l13 8-13 8V4z" />
  </Icon>
);

/** 锤子（构建） */
export const IconHammer = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M15 12l-8.5 8.5a2.12 2.12 0 0 1-3-3L12 9" />
    <path d="M17.64 15L22 10.64" />
    <path d="M20.91 11.7l-1.25-1.25c-.6-.6-.93-1.4-.93-2.25v-.86L16.01 4.6a5.56 5.56 0 0 0-3.94-1.64H9l.92.82A6.18 6.18 0 0 0 12 5.36c.9.3 1.86.3 2.76 0L17 7.38" />
  </Icon>
);

/** 烧瓶（测试） */
export const IconFlask = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M10 2v7.5a2 2 0 0 1-.21.9L4.72 20.55a1 1 0 0 0 .9 1.45h12.76a1 1 0 0 0 .9-1.45L14.21 10.4a2 2 0 0 1-.21-.9V2" />
    <path d="M8.5 2h7M7 16h10" />
  </Icon>
);

/** 垃圾桶（清理缓存） */
export const IconTrash = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
    <path d="M10 11v6M14 11v6" />
  </Icon>
);

/** 右侧检视面板（检视抽屉的开关） */
export const IconPanelRight = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <rect x="3" y="3" width="18" height="18" rx="2.5" />
    <path d="M14 3v18" />
  </Icon>
);

/** 统计（语言构成 / 提交活跃度） */
export const IconChart = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M3 3v16.5A1.5 1.5 0 0 0 4.5 21H21" />
    <path d="M7.5 15.5v-3M12 15.5v-7M16.5 15.5v-5" />
  </Icon>
);

/** 上/下箭头（日志抽屉收展） */
export const IconChevronDown = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <path d="M6 9.5l6 6 6-6" />
  </Icon>
);
