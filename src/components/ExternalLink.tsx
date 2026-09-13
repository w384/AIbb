import type { MouseEvent, ReactNode } from "react";
import { openExternal } from "../lib/tauri";

/**
 * A chat link: left-click opens the URL in the system default browser, and
 * right-click raises the caller-provided link menu instead of the webview's
 * built-in one.
 */
export function ExternalLink({
  href,
  className,
  title,
  children,
  onLinkContextMenu,
}: {
  href: string;
  className?: string;
  title?: string;
  children: ReactNode;
  onLinkContextMenu?: (event: MouseEvent, url: string) => void;
}) {
  return (
    <a
      className={className}
      href={href}
      title={title}
      onClick={(event) => {
        event.preventDefault();
        void openExternal(href);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        onLinkContextMenu?.(event, href);
      }}
    >
      {children}
    </a>
  );
}
