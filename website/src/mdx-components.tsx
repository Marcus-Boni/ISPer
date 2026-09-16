import type { MDXComponents } from "mdx/types";
import Link from "next/link";

export function useMDXComponents(components: MDXComponents): MDXComponents {
  return {
    a: ({ href = "", children, ...props }) => href.startsWith("/") ? <Link href={href} {...props}>{children}</Link> : <a href={href} target={href.startsWith("http") ? "_blank" : undefined} rel={href.startsWith("http") ? "noreferrer" : undefined} {...props}>{children}</a>,
    blockquote: ({ children }) => <aside className="doc-callout">{children}</aside>,
    table: ({ children }) => <div className="doc-table-wrap"><table>{children}</table></div>,
    ...components,
  };
}
