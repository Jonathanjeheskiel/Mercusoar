// The Mercusoar lighthouse mark. Decorative, so screen readers skip it.
export default function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      {/* light beams */}
      <path d="M16 7 L3 3 L3 11 Z" fill="var(--color-amber)" opacity="0.55" />
      <path d="M16 7 L29 3 L29 11 Z" fill="var(--color-amber)" opacity="0.55" />
      {/* lamp */}
      <rect x="13" y="5" width="6" height="5" rx="1" fill="var(--color-amber)" />
      {/* tower */}
      <path d="M12 10 H20 L22 28 H10 Z" fill="var(--color-text)" />
      <path d="M11.4 16 H20.6 L20.9 19 H11.1 Z" fill="var(--color-teal)" />
      {/* sea */}
      <path d="M4 29 Q8 27 12 29 T20 29 T28 29" stroke="var(--color-teal)" strokeWidth="2" fill="none" />
    </svg>
  );
}
