import { RootProvider } from 'fumadocs-ui/provider/next';
import type { ReactNode } from 'react';
import './global.css';

// Dark-only, like the landing page: plotui's identity is the terminal.
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className="dark" suppressHydrationWarning>
      <body className="flex min-h-screen flex-col" suppressHydrationWarning>
        <RootProvider theme={{ enabled: false, forcedTheme: 'dark' }}>{children}</RootProvider>
      </body>
    </html>
  );
}
