const BASE_URL = 'https://justyeet.it/api/v1';

export interface Post {
  id: string;
  content: string;
  media_url: string | null;
  is_adult: boolean;
  is_nft: boolean;
  like_count: number;
  reshare_count: number;
  comment_count: number;
  is_liked: boolean;
  expires_at: string;
  created_at: string;
  author: {
    id: string;
    wallet_address: string;
    display_name: string | null;
    avatar_url: string | null;
  };
}

export interface FeedResponse {
  success: boolean;
  data: Post[];
  total: number;
  page: number;
  per_page: number;
}

async function request<T>(
  path: string,
  options: RequestInit = {},
  token?: string,
): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  const res = await fetch(`${BASE_URL}${path}`, {...options, headers});
  const json = await res.json();
  if (!res.ok) {
    throw new Error(json.error?.message || `HTTP ${res.status}`);
  }
  return json;
}

export const api = {
  health: () => request<{status: string}>('/health'),

  feed: (page = 1, token?: string) =>
    request<FeedResponse>(`/feed?page=${page}&per_page=20`, {}, token),

  feedFollowing: (page = 1, token: string) =>
    request<FeedResponse>(`/feed/following?page=${page}&per_page=20`, {}, token),

  nonce: (address: string) =>
    request<{success: boolean; data: {nonce: string; message: string}}>(
      '/auth/nonce',
      {method: 'POST', body: JSON.stringify({address})},
    ),

  verify: (address: string, nonce: string, signature: string) =>
    request<{success: boolean; data: {access_token: string; refresh_token: string}}>(
      '/auth/verify',
      {method: 'POST', body: JSON.stringify({address, nonce, signature})},
    ),

  createPost: (content: string, token: string) =>
    request<{success: boolean; data: Post}>(
      '/posts',
      {method: 'POST', body: JSON.stringify({content})},
      token,
    ),

  likePost: (id: string, token: string) =>
    request<{success: boolean}>(`/posts/${id}/like`, {method: 'POST'}, token),
};