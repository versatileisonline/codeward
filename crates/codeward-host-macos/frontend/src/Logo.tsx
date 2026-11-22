import logo from "./assets/codeward-logo.png";

export function Logo({ className = "" }: { className?: string }) {
  return (
    <img
      className={`codeward-logo ${className}`}
      src={logo}
      alt="Codeward lock logo"
      width={112}
      height={112}
    />
  );
}
