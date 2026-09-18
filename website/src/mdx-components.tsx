import type { MDXComponents } from "mdx/types";
import Link from "next/link";
import { DocPre } from "@/components/docs/doc-code";
import { DocHeading } from "@/components/docs/doc-heading";

export function useMDXComponents(components: MDXComponents): MDXComponents {
  return {
    a: ({ href = "", children, ...props }) => href.startsWith("/") ? <Link href={href} {...props}>{children}</Link> : <a href={href} target={href.startsWith("http") ? "_blank" : undefined} rel={href.startsWith("http") ? "noreferrer" : undefined} {...props}>{children}</a>,
    blockquote: ({ children }) => <blockquote className="doc-quote">{children}</blockquote>,
    table: ({ children }) => <div className="doc-table-wrap"><table>{children}</table></div>,
    pre: (props) => <DocPre {...props} />,
    h2: (props) => <DocHeading as="h2" {...props} />,
    h3: (props) => <DocHeading as="h3" {...props} />,
    ...components,
  };
}
