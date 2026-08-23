import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "VIC Panel",
  description: "The full-screen private workspace for VIC and Omarchy Voice.",
  openGraph: {
    title: "VIC Panel",
    description: "Private voice intelligence. One continuous conversation.",
    images: [{ url: "/og.png", width: 1536, height: 1024, alt: "VIC Panel" }],
  },
  twitter: {
    card: "summary_large_image",
    title: "VIC Panel",
    description: "Private voice intelligence. One continuous conversation.",
    images: ["/og.png"],
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased`}
      >
        {children}
      </body>
    </html>
  );
}
