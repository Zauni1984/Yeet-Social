import React from 'react';
import {ScrollView, StyleSheet, Text, TouchableOpacity, View} from 'react-native';
import {SafeAreaView} from 'react-native-safe-area-context';
import {Post} from '../api';
import {PostCard} from '../components/PostCard';
import {C, T} from '../theme';

export function PostDetailScreen({route, navigation}: any) {
  const post: Post = route.params.post;
  return (
    <SafeAreaView style={styles.container} edges={['top']}>
      <View style={styles.header}>
        <TouchableOpacity onPress={() => navigation.goBack()} style={styles.back}>
          <Text style={styles.backTxt}>← Back</Text>
        </TouchableOpacity>
        <Text style={styles.title}>Yeet</Text>
        <View style={{width: 60}} />
      </View>
      <ScrollView>
        <PostCard post={post} onPress={() => {}} />
        <View style={styles.replies}>
          <Text style={styles.repliesTxt}>Replies coming soon</Text>
        </View>
      </ScrollView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {flex: 1, backgroundColor: C.bg},
  header: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between',
    paddingHorizontal: 16, paddingVertical: 14,
    borderBottomWidth: 1, borderBottomColor: C.line,
  },
  back: {width: 60},
  backTxt: {color: C.accent, fontFamily: T.mono, fontSize: 12},
  title: {color: '#eef0f6', fontFamily: T.mono, fontSize: 14, fontWeight: '500'},
  replies: {padding: 32, alignItems: 'center'},
  repliesTxt: {color: C.muted, fontFamily: T.mono, fontSize: 12},
});