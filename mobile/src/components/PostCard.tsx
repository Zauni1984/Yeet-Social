import React from 'react';
import {StyleSheet, Text, TouchableOpacity, View} from 'react-native';
import {Post} from '../api';
import {C, T} from '../theme';

function timeAgo(iso: string) {
  const s = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

function expiresIn(iso: string) {
  const s = Math.floor((new Date(iso).getTime() - Date.now()) / 1000);
  if (s <= 0) return 'expired';
  if (s < 3600) return `${Math.floor(s / 60)}m left`;
  return `${Math.floor(s / 3600)}h left`;
}

function expiryPct(created: string, expires: string) {
  const total = new Date(expires).getTime() - new Date(created).getTime();
  const elapsed = Date.now() - new Date(created).getTime();
  return Math.max(0, Math.min(100, 100 - (elapsed / total) * 100));
}

function fmtAddr(a: string) {
  return a ? `${a.slice(0, 6)}…${a.slice(-4)}` : 'anon';
}

const AVATAR_COLORS = ['#c6f135', '#8ef0a2', '#7ecfff', '#ffd17e', '#ff9ef0'];

export function PostCard({post, onPress}: {post: Post; onPress: () => void}) {
  const addr = post.author?.wallet_address ?? '';
  const name = post.author?.display_name ?? fmtAddr(addr);
  const avColor = AVATAR_COLORS[addr ? parseInt(addr.slice(2, 4), 16) % 5 : 0];
  const avInitials = (name || '?').slice(0, 2).toUpperCase();
  const pct = expiryPct(post.created_at, post.expires_at);

  return (
    <TouchableOpacity style={styles.card} onPress={onPress} activeOpacity={0.85}>
      <View style={[styles.avatar, {borderColor: avColor + '44'}]}>
        <Text style={[styles.avatarTxt, {color: avColor}]}>{avInitials}</Text>
      </View>
      <View style={styles.body}>
        {post.is_nft && (
          <View style={styles.nftBadge}>
            <Text style={styles.nftTxt}>◈ NFT</Text>
          </View>
        )}
        <View style={styles.meta}>
          <Text style={styles.name} numberOfLines={1}>{name}</Text>
          <Text style={styles.addr}>{addr ? fmtAddr(addr) : ''}</Text>
          <Text style={styles.time}>{timeAgo(post.created_at)}</Text>
        </View>
        <Text style={styles.content}>{post.content}</Text>
        <View style={styles.expRow}>
          <Text style={styles.expTxt}>⏱ {expiresIn(post.expires_at)}</Text>
          <View style={styles.expBar}>
            <View style={[styles.expFill, {width: `${pct}%`}]} />
          </View>
        </View>
        <View style={styles.actions}>
          <TouchableOpacity style={styles.action}>
            <Text style={styles.actionTxt}>♥ {post.like_count}</Text>
          </TouchableOpacity>
          <TouchableOpacity style={styles.action}>
            <Text style={styles.actionTxt}>◎ {post.comment_count}</Text>
          </TouchableOpacity>
          <TouchableOpacity style={styles.action}>
            <Text style={styles.actionTxt}>↺ {post.reshare_count}</Text>
          </TouchableOpacity>
          <TouchableOpacity style={styles.action}>
            <Text style={styles.actionTxt}>⚡ Tip</Text>
          </TouchableOpacity>
        </View>
      </View>
    </TouchableOpacity>
  );
}

const styles = StyleSheet.create({
  card: {
    flexDirection: 'row', padding: 14, gap: 10,
    borderBottomWidth: 1, borderBottomColor: C.line,
  },
  avatar: {
    width: 38, height: 38, backgroundColor: C.panel,
    borderWidth: 1, alignItems: 'center', justifyContent: 'center', flexShrink: 0,
  },
  avatarTxt: {fontFamily: T.mono, fontSize: 12, fontWeight: '500'},
  body: {flex: 1},
  nftBadge: {
    alignSelf: 'flex-start', borderWidth: 1,
    borderColor: 'rgba(198,241,53,0.25)', backgroundColor: 'rgba(198,241,53,0.07)',
    paddingHorizontal: 7, paddingVertical: 2, marginBottom: 4,
  },
  nftTxt: {color: C.accent, fontFamily: T.mono, fontSize: 9, letterSpacing: 0.8},
  meta: {flexDirection: 'row', alignItems: 'baseline', gap: 6, flexWrap: 'wrap', marginBottom: 4},
  name: {color: '#eef0f6', fontFamily: T.mono, fontSize: 13, fontWeight: '500', flexShrink: 1},
  addr: {color: C.muted, fontFamily: T.mono, fontSize: 10},
  time: {color: C.muted, fontFamily: T.mono, fontSize: 10, marginLeft: 'auto'},
  content: {color: '#d0d4e0', fontFamily: T.mono, fontSize: 13, lineHeight: 20},
  expRow: {flexDirection: 'row', alignItems: 'center', gap: 8, marginTop: 6},
  expTxt: {color: C.muted, fontFamily: T.mono, fontSize: 10},
  expBar: {flex: 1, maxWidth: 80, height: 1, backgroundColor: C.line, overflow: 'hidden'},
  expFill: {height: 1, backgroundColor: C.accent, opacity: 0.5},
  actions: {flexDirection: 'row', marginTop: 10, gap: 0},
  action: {paddingRight: 16, paddingTop: 4},
  actionTxt: {color: C.muted, fontFamily: T.mono, fontSize: 11},
});