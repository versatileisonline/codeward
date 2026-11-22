import logo from "./assets/codeward-guarded-brackets.png";

export function Logo({ className = "" }: { className?: string }) {
  return (
    <img
      className={`codeward-logo ${className}`}
      src={logo}
      alt="Codeward lock logo"
      width={160}
      height={100}
    />
  );
}
