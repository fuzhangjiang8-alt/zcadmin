// =============================================================================
// 审计告警规则：查看 / 新增 / 编辑 / 删除 / 全量保存。
//
// 规则匹配使用 SQL LIKE，`*` 表示全部；后端要求必须保留一条 `*` 规则。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';

class AlertRulesPage extends StatefulWidget {
  const AlertRulesPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<AlertRulesPage> createState() => _AlertRulesPageState();
}

class _AlertRulesPageState extends State<AlertRulesPage> {
  bool _loading = true;
  bool _saving = false;
  String? _error;
  List<Map<String, dynamic>> _rules = [];

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final rules = await widget.api.alertRules();
      if (!mounted) return;
      setState(() {
        _rules = rules;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _save() async {
    if (!_rules.any((r) => r['actionPattern'] == '*')) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('必须保留一条 * 通配规则')),
      );
      return;
    }
    setState(() => _saving = true);
    try {
      await widget.api.saveAlertRules(_rules);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('规则已保存')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _edit({int? index}) async {
    final existing = index == null ? null : _rules[index];
    final pattern = TextEditingController(
      text: existing == null ? '' : existing['actionPattern']?.toString() ?? '',
    );
    final window = TextEditingController(
      text: existing == null ? '5' : existing['windowMinutes']?.toString() ?? '5',
    );
    final threshold = TextEditingController(
      text: existing == null ? '50' : existing['threshold']?.toString() ?? '50',
    );
    final silence = TextEditingController(
      text: existing == null ? '0' : existing['silenceMinutes']?.toString() ?? '0',
    );
    var enabled = existing == null ? true : existing['enabled'] as bool? ?? true;

    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: Text(index == null ? '新增规则' : '编辑规则'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: pattern,
                  decoration: const InputDecoration(
                    labelText: '动作模式',
                    hintText: 'auth.login.failed 或 *',
                  ),
                ),
                TextField(
                  controller: window,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(labelText: '窗口（分钟，1-60）'),
                ),
                TextField(
                  controller: threshold,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(labelText: '阈值（1-10000）'),
                ),
                TextField(
                  controller: silence,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(labelText: '静默（分钟，0-1440）'),
                ),
                SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  title: const Text('启用'),
                  value: enabled,
                  onChanged: (v) => setDialogState(() => enabled = v),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('确定'),
            ),
          ],
        ),
      ),
    );
    if (saved != true) return;

    final p = pattern.text.trim();
    final w = int.tryParse(window.text.trim());
    final t = int.tryParse(threshold.text.trim());
    final s = int.tryParse(silence.text.trim());
    if (p.isEmpty ||
        w == null ||
        w < 1 ||
        w > 60 ||
        t == null ||
        t < 1 ||
        t > 10000 ||
        s == null ||
        s < 0 ||
        s > 1440) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('请检查动作模式与数值范围')),
        );
      }
      return;
    }
    final rule = {
      'actionPattern': p,
      'windowMinutes': w,
      'threshold': t,
      'silenceMinutes': s,
      'enabled': enabled,
    };
    setState(() {
      if (index == null) {
        _rules.add(rule);
      } else {
        _rules[index] = rule;
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('告警规则'),
        actions: [
          IconButton(
            tooltip: '新增',
            icon: const Icon(Icons.add),
            onPressed: () => _edit(),
          ),
          TextButton(
            onPressed: _saving ? null : _save,
            child: Text(_saving ? '保存中' : '保存'),
          ),
        ],
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : _error != null
              ? Center(child: Text(_error!))
              : _rules.isEmpty
                  ? const Center(child: Text('暂无规则'))
                  : ListView.separated(
                      itemCount: _rules.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, index) {
                        final r = _rules[index];
                        return ListTile(
                          title: Text('${r['actionPattern']}'),
                          subtitle: Text(
                            '窗口 ${r['windowMinutes']} 分钟 · 阈值 ${r['threshold']}'
                            ' · 静默 ${r['silenceMinutes']} 分钟',
                          ),
                          leading: Icon(
                            (r['enabled'] as bool? ?? true)
                                ? Icons.notifications_active
                                : Icons.notifications_off,
                            color: (r['enabled'] as bool? ?? true)
                                ? Colors.green
                                : Colors.grey,
                          ),
                          onTap: () => _edit(index: index),
                          trailing: IconButton(
                            icon: const Icon(Icons.delete_outline),
                            onPressed: () => setState(() => _rules.removeAt(index)),
                          ),
                        );
                      },
                    ),
    );
  }
}
