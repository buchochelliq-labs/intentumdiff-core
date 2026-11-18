# Extract Puppet manifests from puppet's own parser specs (core #267 / #268 ledger).
#
# Benchmark tooling only. Reads a spec file with Ripper (no spec code is executed) and
# prints one JSON line per `parse(<literal>)` call whose argument is a plain string
# literal without interpolation, with the spec's own expectation for it:
#   accept - inside `expect(...)` matched with anything but raise_error, or inside
#            `expect { ... }.not_to raise_error`
#   reject - inside `expect { ... }.to raise_error(...)`
# Other calls are reported with expectation "unknown" and are not admitted.
#
#   ruby extract_specs.rb SPEC.rb
require 'ripper'
require 'json'

def ident_name(node)
  return nil unless node.is_a?(Array)
  case node[0]
  when :@ident then node[1]
  when :fcall, :vcall, :var_ref then ident_name(node[1])
  when :method_add_arg, :method_add_block then ident_name(node[1])
  end
end

def contains_ident?(node, name)
  return false unless node.is_a?(Array)
  return true if node[0] == :@ident && node[1] == name
  node.any? { |child| contains_ident?(child, name) }
end

# [verb, matcher_args, receiver] for `<receiver>.to/.not_to/.to_not <matcher>`
def expectation(node)
  return nil unless node.is_a?(Array)
  if node[0] == :command_call || node[0] == :call
    recv, meth = node[1], node[3]
    verb = meth.is_a?(Array) && meth[0] == :@ident ? meth[1] : nil
    return [verb, node[4], recv] if %w[to not_to to_not].include?(verb)
  elsif node[0] == :method_add_arg && node[1].is_a?(Array) && node[1][0] == :call
    inner = expectation(node[1])
    return [inner[0], node[2], inner[2]] if inner
  end
  nil
end

def literal_value(arg, source_lines)
  return nil unless arg.is_a?(Array) && arg[0] == :string_literal
  content = arg[1]
  return nil unless content.is_a?(Array) && content[0] == :string_content
  parts = content[1..]
  return '' if parts.empty?
  return nil unless parts.size == 1 && parts[0][0] == :@tstring_content
  tok = parts[0]
  [tok[1], tok[2]]
end

$records = []

def walk(node, context, src)
  return unless node.is_a?(Array)
  exp = expectation(node)
  if exp
    verb, matcher, recv = exp
    raising = contains_ident?(matcher, 'raise_error')
    ctx = if raising
            verb == 'to' ? 'reject' : 'accept'
          else
            'accept'
          end
    walk(recv, ctx, src)
    walk(matcher, context, src)
    return
  end
  if (node[0] == :method_add_arg) && ident_name(node[1]) == 'parse' && node[1][0] == :fcall
    args = node[2]
    args = args[1] if args.is_a?(Array) && args[0] == :arg_paren
    list = args.is_a?(Array) && args[0] == :args_add_block ? args[1] : nil
    if list.is_a?(Array) && list.size == 1
      lit = literal_value(list[0], src)
      if lit
        raw, (line, _col) = lit
        $records << {line: line, raw: raw, expectation: context || 'unknown'}
      end
    end
  end
  node.each { |child| walk(child, context, src) }
end

path = ARGV.fetch(0)
source = File.read(path, encoding: 'UTF-8')
tree = Ripper.sexp(source) or abort("ripper failed: #{path}")
walk(tree, nil, source.lines)
lexed = Ripper.lex(source)
$records.each do |rec|
  # The opening quote of the literal is the token just before its content.
  idx = lexed.index { |(pos, type, tok)| pos[0] == rec[:line] && type == :on_tstring_content && tok == rec[:raw] }
  opener = idx && lexed[idx - 1][1] == :on_tstring_beg ? lexed[idx - 1][2] : nil
  value = case opener
          when "'" then rec[:raw].gsub(/\\([\\'])/, '\1')
          when '"' then (rec[:raw].include?('#') ? nil : eval('"' + rec[:raw] + '"')) # no interpolation possible
          end
  next if value.nil?
  puts JSON.generate({line: rec[:line], quote: opener, raw: rec[:raw], value: value, expectation: rec[:expectation]})
end
