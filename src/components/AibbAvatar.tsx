interface AibbAvatarProps {
  avatarDataUrl?: string | null;
  className?: string;
  name?: string;
}

export function AibbAvatar({ avatarDataUrl, className = "", name = "AIbb" }: AibbAvatarProps) {
  const avatarClassName = `aibb-avatar ${className}`.trim();
  if (avatarDataUrl) {
    return (
      <img
        alt={name}
        className={avatarClassName}
        src={avatarDataUrl}
        style={{ borderRadius: "50%", overflow: "hidden" }}
      />
    );
  }

  return (
    <svg
      aria-hidden="true"
      className={avatarClassName}
      focusable="false"
      viewBox="0 0 72 72"
    >
      <defs>
        <linearGradient id="aibb-orb" x1="10" y1="8" x2="62" y2="66" gradientUnits="userSpaceOnUse">
          <stop stopColor="#e7f7ff" />
          <stop offset="1" stopColor="#9ccfff" />
        </linearGradient>
        <linearGradient id="aibb-shell" x1="24" y1="23" x2="49" y2="54" gradientUnits="userSpaceOnUse">
          <stop stopColor="#f5efff" />
          <stop offset="1" stopColor="#cda8ee" />
        </linearGradient>
        <linearGradient id="aibb-eye" x1="0" y1="0" x2="0" y2="1">
          <stop stopColor="#65e4ff" />
          <stop offset="1" stopColor="#198bdc" />
        </linearGradient>
      </defs>
      <circle cx="36" cy="36" r="34" fill="url(#aibb-orb)" />
      <path d="M36 15v6" stroke="#e34d7e" strokeWidth="2.5" strokeLinecap="round" />
      <circle cx="36" cy="13" r="3" fill="#f25a8b" />
      <rect x="19" y="29" width="6" height="18" rx="3" fill="#ed4f91" />
      <rect x="47" y="29" width="6" height="18" rx="3" fill="#ed4f91" />
      <rect x="22" y="21" width="28" height="35" rx="12" fill="url(#aibb-shell)" />
      <rect x="25" y="29" width="22" height="14" rx="6" fill="#272445" />
      <rect x="29" y="32" width="4" height="8" rx="2" fill="url(#aibb-eye)" />
      <rect x="39" y="32" width="4" height="8" rx="2" fill="url(#aibb-eye)" />
      <rect x="31" y="48" width="10" height="3.5" rx="1.75" fill="#45365f" />
      <rect x="31" y="18" width="10" height="3" rx="1.5" fill="#f0ad22" />
    </svg>
  );
}
