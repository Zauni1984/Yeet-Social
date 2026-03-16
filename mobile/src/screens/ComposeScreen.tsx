import React, {useState} from 'react';
import {
  Alert, StyleSheet, Text, TextInput,
  TouchableOpacity, View,
} from 'react-native';
import {SafeAreaView} from 'react-native-safe-area-context';
import {api} from '../api';
import {useAuth} from '../context/AuthContext';
import {C, T} from '../theme';

const MAX = 280;
const CIRC = 2 * Math.PI * 12;

export function ComposeScreen({navigation}: any) {
  const {token, isConnected} = useAuth();
  const [text, setText] = useState('');
  const [loading, setLoading] = useState(false);

  const submit = async () => {
    if (!isConnected || !token) {
      Alert.alert('Connect Wallet', 'Please connect your wallet first.');
      return;
    }
    if (!text.trim()) return;
    setLoading(true);
    try {
      await api.createPost(text.trim(), token);
      setText('');
      navigation.navigate('Feed');
    } catch (e: any) {
      Alert.alert('Error', e.message);
    } finally {
      setLoading(false);
    }
  };

  const left = MAX - text.length;
  const pct = text.length / MAX;

  return (
    <SafeAreaView style={styles.container} edges={['top']}>
      <View style={styles.header}>
        <Text style={styles.title}>New Yeet</Text>
        <Text style={styles.note}>Disappears in 24h</Text>
      </View>
      <View style={styles.compose}>
        <View style={styles.avatar}>
          <Text style={styles.avTxt}>YT</Text>
        </View>
        <TextInput
          style={styles.input}
          placeholder="What are you yeeting?"
          placeholderTextColor={C.muted}
          multiline
          maxLength={MAX}
          value={text}
          onChangeText={setText}
          autoFocus
        />
      </View>
      <View style={styles.footer}>
        <View style={styles.tools}>
          {['🖼', '◈', '🔒', '⚡'].map(t => (
            <TouchableOpacity key={t} style={styles.tool}>
              <Text style={styles.toolTxt}>{t}</Text>
            </TouchableOpacity>
          ))}
        </View>
        <View style={styles.right}>
          <Text style={[styles.counter, left < 20 && {color: C.red}]}>
            {left < 20 ? left : ''}
          </Text>
          <TouchableOpacity
            style={[styles.btn, (!text.trim() || loading) && styles.btnDisabled]}
            onPress={submit}
            disabled={!text.trim() || loading}>
            <Text style={styles.btnTxt}>{loading ? '…' : 'Yeet it'}</Text>
          </TouchableOpacity>
        </View>
      </View>
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
  title: {color: '#eef0f6', fontFamily: T.mono, fontSize: 14, fontWeight: '500'},
  note: {color: C.muted, fontFamily: T.mono, fontSize: 11},
  compose: {flexDirection: 'row', padding: 16, gap: 12, flex: 1},
  avatar: {
    width: 36, height: 36, backgroundColor: C.panel,
    borderWidth: 1, borderColor: 'rgba(198,241,53,0.4)',
    alignItems: 'center', justifyContent: 'center', flexShrink: 0,
  },
  avTxt: {color: C.accent, fontFamily: T.mono, fontSize: 11},
  input: {
    flex: 1, color: '#eef0f6', fontFamily: T.mono,
    fontSize: 14, lineHeight: 22, textAlignVertical: 'top',
  },
  footer: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between',
    paddingHorizontal: 16, paddingVertical: 12,
    borderTopWidth: 1, borderTopColor: C.line,
  },
  tools: {flexDirection: 'row', gap: 4},
  tool: {padding: 8},
  toolTxt: {color: C.muted, fontSize: 15},
  right: {flexDirection: 'row', alignItems: 'center', gap: 10},
  counter: {color: C.muted, fontFamily: T.mono, fontSize: 10},
  btn: {backgroundColor: C.accent, paddingHorizontal: 18, paddingVertical: 8},
  btnDisabled: {opacity: 0.4},
  btnTxt: {color: '#060801', fontFamily: T.mono, fontSize: 11, fontWeight: '500', textTransform: 'uppercase', letterSpacing: 0.8},
});