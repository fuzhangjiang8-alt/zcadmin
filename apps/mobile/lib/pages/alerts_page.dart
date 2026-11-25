// =============================================================================
// 审计告警：全部未处理告警 / 我的订阅告警 + 一键处理。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'alert_rules_page.dart';

class AlertsPage extends StatefulWidget {
  const AlertsPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<AlertsPage> createState() => _AlertsPageState();
}

class _AlertsPageState extends State<AlertsPage> {
  bool _mine = false;
  late Future<List<Map<String, dynamic>>> _future = _load();

  Future<List<Map<String, dynamic>>> _load() =>
      _mine ? widget.api.subscribedAlerts() : widget.api.auditAlerts();

  Future<void> _refresh() async {
    setState(() => _future = _load());
    await _future;
  }

  Future<void> _ack(String id) async {
    try {
      await widget.api.ackAuditAlert(id);
      await _refresh();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('已处理')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
      }
    }
  }

  Future<void> _editSubscriptions() async {
    try {
      final patterns = await widget.api.subscriptions();
      if (!mounted) return;
      final controller = TextEditingController(text: patterns.join(', '));
      final saved = await showDialog<bool>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: const Text('我的告警订阅'),
          content: TextField(
            controller: controller,
            maxLines: 4,
            decoration: const InputDecoration(
              hintText: 'auth.login.failed, system.*',
              helperText: '逗号分隔，* 表示全部',
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('取消')),
            FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('保存')),
          ],
        ),
      );
      if (saved == true) {
        final list = controller.text
            .split(',')
            .map((e) => e.trim())
            .where((e) => e.isNotEmpty)
            .toList();
        await widget.api.saveSubscriptions(list);
        if (!mounted) return;
        await _refresh();
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('订阅已保存')));
        }
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('审计告警'),
        actions: [
          IconButton(
            tooltip: '告警规则',
            icon: const Icon(Icons.rule),
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute(
                builder: (_) => AlertRulesPage(api: widget.api),
              ),
            ),
          ),
          IconButton(
            tooltip: '订阅设置',
            icon: const Icon(Icons.tune),
            onPressed: _editSubscriptions,
          ),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.all(12),
            child: SegmentedButton<bool>(
              segments: const [
                ButtonSegment(value: false, label: Text('全部')),
                ButtonSegment(value: true, label: Text('我的订阅')),
              ],
              selected: {_mine},
              onSelectionChanged: (value) {
                setState(() {
                  _mine = value.first;
                  _future = _load();
                });
              },
            ),
          ),
          Expanded(
            child: RefreshIndicator(
              onRefresh: _refresh,
              child: FutureBuilder<List<Map<String, dynamic>>>(
                future: _future,
                builder: (context, snapshot) {
                  if (snapshot.connectionState != ConnectionState.done) {
                    return const Center(child: CircularProgressIndicator());
                  }
                  if (snapshot.hasError) {
                    return Center(child: Text(snapshot.error.toString()));
                  }
                  final rows = snapshot.data ?? [];
                  if (rows.isEmpty) {
                    return Center(child: Text(_mine ? '暂无订阅告警' : '暂无未处理告警'));
                  }
                  return ListView.separated(
                    itemCount: rows.length,
                    separatorBuilder: (_, __) => const Divider(height: 1),
                    itemBuilder: (context, index) {
                      final a = rows[index];
                      return ListTile(
                        leading: const Icon(Icons.warning_amber, color: Colors.orange),
                        title: Text('${a['action']}'),
                        subtitle: Text('触发 ${a['count']} 次 · ${a['windowStart']}'),
                        trailing: TextButton(
                          onPressed: () => _ack('${a['id']}'),
                          child: const Text('处理'),
                        ),
                      );
                    },
                  );
                },
              ),
            ),
          ),
        ],
      ),
    );
  }
}
