export interface ExtMarketplaceProvider {
  id: string;
  name: string;
  category: string;
  provider: unknown;
  builtin: boolean;
  installed: boolean;
}

export interface ExtMarketplaceResult {
  providers: ExtMarketplaceProvider[];
  installed_count: number;
}

export interface ExtInstallResult {
  id: string;
  installed: boolean;
}

export interface ExtUninstallResult {
  id: string;
  uninstalled: boolean;
}