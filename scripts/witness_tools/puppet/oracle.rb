# Puppet reference oracle for the #267 family (core #268 ledger). Runs the released
# puppet gem itself, never IntentumDiff.
#
#   ruby oracle.rb parse FILE     Puppet::Pops::Parser::Parser#parse_string, as the parser
#                                 specs' `parse` helper calls it: {accepts, error}
#   ruby oracle.rb catalog FILE   compile a catalog for a fixed node with fixed facts and
#                                 print a normalised projection: every resource with its
#                                 non-relationship parameters, plus the ordering and
#                                 refresh edge sets from metaparameters and arrows.
require 'json'
require 'tmpdir'
require 'puppet'

mode, path = ARGV
source = File.read(path, encoding: 'UTF-8')
result = {tool: "puppet #{Puppet.version}", ruby: RUBY_VERSION}

Dir.mktmpdir('puppet-witness') do |dir|
  Puppet.initialize_settings(['--confdir', dir, '--vardir', "#{dir}/var", '--codedir', "#{dir}/code",
                              '--logdir', "#{dir}/log", '--rundir', "#{dir}/run"])
  case mode
  when 'parse'
    begin
      Puppet::Pops::Parser::Parser.new.parse_string(source)
      result[:accepts] = true
    rescue StandardError => e
      result[:accepts] = false
      result[:error] = "#{e.class}: #{e.message}"
    end
  when 'catalog'
    begin
      Puppet[:code] = source
      facts = Puppet::Node::Facts.new('witness.example', {'os' => {'family' => 'Debian'}, 'networking' => {'fqdn' => 'witness.example'}})
      node = Puppet::Node.new('witness.example', facts: facts)
      catalog = Puppet::Parser::Compiler.new(node).compile
      rel = %i[before require notify subscribe]
      resources = {}
      order, refresh = [], []
      catalog.resources.each do |r|
        resources[r.ref] = r.to_hash.reject { |k, _| rel.include?(k) }.map { |k, v| [k.to_s, v.inspect] }.sort.to_h
        rel.each do |meta|
          value = r[meta]
          value = [value] unless value.is_a?(Array)
          value.flatten.compact.each do |other|
            other = other.respond_to?(:ref) ? other.ref : other.to_s
            case meta
            when :before then order << [r.ref, other]
            when :require then order << [other, r.ref]
            when :notify then refresh << [r.ref, other]
            when :subscribe then refresh << [other, r.ref]
            end
          end
        end
      end
      # A refresh edge also orders its endpoints.
      result[:accepts] = true
      result[:projection] = {resources: resources.sort.to_h, ordering: (order + refresh).uniq.sort,
                             refresh: refresh.uniq.sort}
    rescue StandardError => e
      result[:accepts] = false
      result[:error] = "#{e.class}: #{e.message}"
    end
  else
    abort 'usage: oracle.rb parse|catalog FILE'
  end
end
puts JSON.pretty_generate(result)
