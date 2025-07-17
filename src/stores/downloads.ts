import { defineStore } from 'pinia';
import { ref } from 'vue';

export interface Download {
  id: string;
  name: string;
  size: number;
  progress: number;
  status: 'queued' | 'downloading' | 'completed' | 'error' | 'paused';
  eta?: number;
  speed?: number;
  error?: string;
}

export const useDownloadsStore = defineStore('downloads', () => {
  const downloads = ref<Download[]>([]);
  const activeDownloads = ref(0);

  const addDownload = (download: Download) => {
    downloads.value.push(download);
    if (download.status === 'downloading') {
      activeDownloads.value++;
    }
  };

  const updateDownload = (id: string, updates: Partial<Download>) => {
    const index = downloads.value.findIndex(d => d.id === id);
    if (index !== -1) {
      const oldStatus = downloads.value[index].status;
      downloads.value[index] = { ...downloads.value[index], ...updates };
      
      // Update active download count
      if (oldStatus === 'downloading' && updates.status !== 'downloading') {
        activeDownloads.value--;
      } else if (oldStatus !== 'downloading' && updates.status === 'downloading') {
        activeDownloads.value++;
      }
    }
  };

  const removeDownload = (id: string) => {
    const index = downloads.value.findIndex(d => d.id === id);
    if (index !== -1) {
      if (downloads.value[index].status === 'downloading') {
        activeDownloads.value--;
      }
      downloads.value.splice(index, 1);
    }
  };

  const getActiveDownloads = () => {
    return downloads.value.filter(d => d.status === 'downloading');
  };

  return {
    downloads,
    activeDownloads,
    addDownload,
    updateDownload,
    removeDownload,
    getActiveDownloads
  };
});