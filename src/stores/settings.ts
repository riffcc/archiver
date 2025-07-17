import { defineStore } from 'pinia';
import { ref } from 'vue';

export interface AppSettings {
  theme: 'dark' | 'light';
  animationMode: 'full' | 'low' | 'none';
  archiveOrgAuth?: {
    clientId?: string;
    clientSecret?: string;
    accessToken?: string;
    refreshToken?: string;
  };
  musicBrainzAuth?: {
    clientId?: string;
    clientSecret?: string;
    accessToken?: string;
  };
  qBittorrent?: {
    url: string;
    username?: string;
    password?: string;
  };
  importPaths?: {
    processing: string;
    library: string;
  };
}

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<AppSettings>({
    theme: 'dark',
    animationMode: 'full',
    importPaths: {
      processing: '/mnt/mfs/riffcc/processing',
      library: '~/.riffcc/library'
    }
  });

  const updateSettings = (newSettings: Partial<AppSettings>) => {
    settings.value = { ...settings.value, ...newSettings };
  };

  const toggleTheme = () => {
    settings.value.theme = settings.value.theme === 'dark' ? 'light' : 'dark';
  };

  return {
    settings,
    updateSettings,
    toggleTheme
  };
});