import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

const navTitle = (
  <span className="plotui-nav-title" aria-label="plotui documentation">
    plot<span className="plotui-nav-accent">ui</span>
  </span>
);

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: navTitle,
      url: '/docs',
    },
    links: [
      {
        text: 'Examples',
        url: 'https://plotui.fly.dev/examples.html',
        external: true,
      },
      {
        text: 'plotui.fly.dev',
        url: 'https://plotui.fly.dev',
        external: true,
      },
    ],
  };
}
