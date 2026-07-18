// Minimal line icons. Single stroke, currentColor, no fill — calm and flat.

type P = { className?: string };
const base = "w-6 h-6";

export const ShieldIcon = ({ className = base }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 3l7 3v5c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6l7-3z" />
    <path d="M9 12l2 2 4-4" />
  </svg>
);

export const ClockIcon = ({ className = base }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 2" />
  </svg>
);

export const ListIcon = ({ className = base }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
    <path d="M8 6h12M8 12h12M8 18h12" />
    <path d="M4 6h.01M4 12h.01M4 18h.01" />
  </svg>
);

export const ChartIcon = ({ className = base }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
    <path d="M4 20V10M10 20V4M16 20v-7M22 20H2" />
  </svg>
);

export const HeartIcon = ({ className = base }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 20s-7-4.35-9-8.5C1.5 8 3 5 6 5c2 0 3 1.2 3.75 2.25C10.5 6.2 11.5 5 13.5 5c3 0 4.5 3 3 6.5-2 4.15-9 8.5-9 8.5z" />
  </svg>
);

export const CheckIcon = ({ className = base }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M5 12.5l4.5 4.5L19 6.5" />
  </svg>
);

export const BackIcon = ({ className = "w-5 h-5" }: P) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M15 6l-6 6 6 6" />
  </svg>
);
