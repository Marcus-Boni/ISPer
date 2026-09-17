import { Link2 } from "lucide-react";

/**
 * Headings carry their own link. rehype-slug already puts an id on every one;
 * without a visible anchor the reader has no way to send someone to a specific
 * step, which on a troubleshooting page is most of what sharing means.
 *
 * The anchor is a pointer affordance only. Inside the heading it would otherwise
 * join the heading's accessible name — "Antes de instalar, Link para esta seção"
 * — and add a tab stop duplicating a link the on-this-page rail already offers
 * to keyboard users.
 */
export function DocHeading({ as: Tag, id, children, ...props }: { as: "h2" | "h3"; id?: string } & React.ComponentProps<"h2">) {
  if (!id) return <Tag {...props}>{children}</Tag>;
  return (
    <Tag id={id} {...props}>
      {children}
      <a className="heading-anchor" href={`#${id}`} aria-hidden="true" tabIndex={-1}>
        <Link2 />
      </a>
    </Tag>
  );
}
