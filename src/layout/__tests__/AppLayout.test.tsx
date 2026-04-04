import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { AppLayout } from "../AppLayout";

// Mock child components to isolate AppLayout structure
vi.mock("../../components/UpdateDialog", () => ({
  UpdateDialog: () => <div data-testid="update-dialog">update-dialog</div>,
}));

vi.mock("../../components/KeywordReviewDialog", () => ({
  KeywordReviewDialog: () => <div data-testid="keyword-review-dialog">keyword-review-dialog</div>,
}));

vi.mock("../../ui/Sidebar", () => ({
  Sidebar: ({ isOpen }: { isOpen: boolean }) => (
    <aside data-testid="sidebar">sidebar-open:{String(isOpen)}</aside>
  ),
}));

vi.mock("../../ui/MobileNav", () => ({
  MobileNav: ({ isOpen }: { isOpen: boolean }) => (
    <div data-testid="mobile-nav">mobile-nav-open:{String(isOpen)}</div>
  ),
  MobileHeader: ({ onMenuClick }: { onMenuClick: () => void }) => (
    <header data-testid="mobile-header">
      <button type="button" onClick={onMenuClick}>
        menu
      </button>
    </header>
  ),
}));

vi.mock("../../hooks/useMediaQuery", () => ({
  useResponsive: () => ({
    isMobile: false,
    isTablet: false,
    isDesktop: true,
    isLargeDesktop: false,
    shouldShowSidebar: true,
    shouldShowMobileNav: false,
  }),
}));

vi.mock("../../hooks/useSidebarState", () => ({
  useSidebarState: () => ({
    isOpen: true,
    isMobileDrawerOpen: false,
    toggle: vi.fn(),
    open: vi.fn(),
    close: vi.fn(),
    toggleMobileDrawer: vi.fn(),
    openMobileDrawer: vi.fn(),
    closeMobileDrawer: vi.fn(),
  }),
}));

describe("layout/AppLayout", () => {
  it("renders sidebar, main content area (Outlet), and UpdateDialog on desktop", () => {
    render(
      <MemoryRouter>
        <AppLayout />
      </MemoryRouter>
    );

    expect(screen.getByTestId("sidebar")).toBeInTheDocument();
    expect(screen.getByTestId("update-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("mobile-nav")).toBeInTheDocument();
    // Tauri drag region
    expect(document.querySelector("[data-tauri-drag-region]")).toBeInTheDocument();
  });

  it("does not render MobileHeader when isDesktop is true", () => {
    render(
      <MemoryRouter>
        <AppLayout />
      </MemoryRouter>
    );

    // MobileHeader is conditionally rendered only when !isDesktop
    expect(screen.queryByTestId("mobile-header")).not.toBeInTheDocument();
  });
});
