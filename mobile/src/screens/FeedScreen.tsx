import React, {useCallback, useEffect, useState} from 'react';
import {
  ActivityIndicator, FlatList, RefreshControl,
  StyleSheet, Text, TouchableOpacity, View,
} from 'react-native';
import {SafeAreaView} from 'react-native-safe-area-context';
import {api, Post} from '../api';
import {useAuth} from '../context/AuthContext';
import {PostCard} from '../components/PostCard';
import {C, T} from '../theme';

export function FeedScreen({navigation}: any) {
  const {token} = useAuth();
  const [posts, setPosts] = useState<Post[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [tab, setTab] = useState<'global' | 'following'>('global');

  const load = useCallback(async () => {
    try {
      const res = tab === 'global'
        ? await api.feed(1, token ?? undefined)
        : token ? await api.feedFollowing(1, token) : null;
      if (res) setPosts(res.data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [tab, token]);

  useEffect(() => { setLoading(true); load(); }, [load]);

  const onRefresh = () => { setRefreshing(true); load(); };

  return (
    <SafeAreaView style={styles.container} edges={['top']}>
      {/* Header */}
      <View style={styles.header}>
        <Text style={styles.logo}>● YEET</Text>
      </View>

      {/* Tabs */}
      <View style={styles.tabs}>
        {(['global', 'following'] as const).map(t => (
          <TouchableOpacity key={t} style={styles.tab} onPress={() => setTab(t)}>
            <Text style={[styles.tabTxt, tab === t && styles.tabActive]}>
              {t === 'global' ? 'For You' : 'Following'}
            </Text>
            {tab === t && <View style={styles.tabLine} />}
          </TouchableOpacity>
        ))}
        <TouchableOpacity style={styles.tab}>
          <Text style={styles.tabTxt}>NFT ◈</Text>
        </TouchableOpacity>
        <TouchableOpacity style={styles.tab}>
          <Text style={styles.tabTxt}>18+ 🔥</Text>
        </TouchableOpacity>
      </View>

      {loading ? (
        <ActivityIndicator color={C.accent} style={{marginTop: 40}} />
      ) : (
        <FlatList
          data={posts}
          keyExtractor={p => p.id}
          renderItem={({item}) => (
            <PostCard
              post={item}
              onPress={() => navigation.navigate('PostDetail', {post: item})}
            />
          )}
          refreshControl={
            <RefreshControl refreshing={refreshing} onRefresh={onRefresh} tintColor={C.accent} />
          }
          ListEmptyComponent={
            <Text style={styles.empty}>No posts yet. Be the first to Yeet!</Text>
          }
          contentContainerStyle={{paddingBottom: 20}}
        />
      )}
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {flex: 1, backgroundColor: C.bg},
  header: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between',
    paddingHorizontal: 16, paddingVertical: 12,
    borderBottomWidth: 1, borderBottomColor: C.line,
  },
  logo: {color: C.accent, fontFamily: T.display, fontSize: 20, fontWeight: '800', letterSpacing: -0.5},
  tabs: {flexDirection: 'row', borderBottomWidth: 1, borderBottomColor: C.line},
  tab: {flex: 1, alignItems: 'center', paddingVertical: 12, position: 'relative'},
  tabTxt: {color: C.muted, fontFamily: T.mono, fontSize: 10, textTransform: 'uppercase', letterSpacing: 0.8},
  tabActive: {color: C.accent},
  tabLine: {position: 'absolute', bottom: 0, left: 0, right: 0, height: 2, backgroundColor: C.accent},
  empty: {color: C.muted, textAlign: 'center', marginTop: 60, fontFamily: T.mono},
});