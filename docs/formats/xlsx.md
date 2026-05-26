# 格式：Excel（.xlsx）网格编辑与写回

## 1. 目标
- 打开 xlsx 并提供高性能数据网格。
- 支持保留公式与基础格式，尽量只写回改动单元格。

## 2. 内部表示
- workbook / sheets
- cell：value / formula / style
- selection range

## 3. 写回策略
- 保存前生成 `.bak`。
- 只写回变更单元格与必要样式。
- 图表/宏/复杂条件格式：尽量保留，无法保证时提示。

## 4. AI 操作数据
- 对网格的 AI 修改必须生成 ChangeSet 并可预览与回滚。
- 对排序/批量格式变更必须二次确认。
