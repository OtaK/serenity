// The `quote!` macro recurses a lot.
#![recursion_limit = "128"]

extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use syn::{
    parse::{Error, Parse, ParseStream, Result},
    parse_macro_input,
    spanned::Spanned,
    Ident, Lit,
};

pub(crate) mod attributes;
pub(crate) mod consts;
pub(crate) mod structures;
pub(crate) mod util;

use attributes::*;
use consts::*;
use structures::*;
use util::*;

macro_rules! match_options {
    ($v:expr, $values:ident, $options:ident, $span:expr => [$($name:ident);*]) => {
        match $v {
            $(
                stringify!($name) => $options.$name.parse(stringify!($name), $values),
            )*
            _ => {
                return Error::new($span, &format!("invalid attribute: {:?}", $v))
                    .to_compile_error()
                    .into();
            },
        }
    };
}

/// The heart of the attribute-based framework.
///
/// This is a function attribute macro; if you attempt to use this on other Rust constructs, it won't work.
///
/// # Options
///
/// To alter how the framework will interpret the command,
/// you can provide options as attributes following this `#[command]` macro.
///
/// Each option has its own kind of data to stock and manipulate with.
/// They're given to the option either with the `#[option(...)]` or `#[option = ...]` syntaxes.
/// If an option doesn't require for any data to be supplied, then it's simply `#[option]`.
///
/// If the input to the option is malformed, the macro will give you can error, describing
/// the correct method for passing data, and what it should be.
///
/// The list of available options, is, as follows:
///
/// - `#[checks(idents)]`
/// Preconditions that must be met. Executed before the command's execution.
/// `idents` is a list of identifiers, seperated by a comma, referencing functions of the declaration:
/// `fn(&mut Context, &Message, &mut Args, &CommandOptions) -> bool`
///
/// - `#[aliases(names)]`
/// A list of other names that can be used to execute this command.
/// In `serenity::framework::standard::CommandOptions`, these are put in the `names` field, right after the command's name.
///
/// - `#[description(desc)]`/`#[description = desc]`
/// A summary of the command.
///
/// - `#[usage(usg)]`/`#[usage = usg]
/// Usage schema of the command.
///
/// - `#[example(ex)]`/`#[example = ex]
/// Example of the command's usage.
///
/// - `#[min_args(min)]`, `#[max_args(max)]`, `#[num_args(min_and_max)]`
/// The minimum and/or maximum amount of arguments that the command should/can receive.
///
/// `num_args` is a helper attribute, serving as a shorthand for calling
/// `min_args` and `max_args` with the same amount of arguments.
///
/// - `#[allowed_roles(roles)]`
/// A list of strings (role names), seperated by a comma,
/// stating that only members of certain roles can execute this command.
///
/// - `#[help_available]`/`#[help_available(bool)]`
/// Whether this command should be displayed in the help message.
///
/// - `#[only_in(context)]`
/// Which context the command can only be executed in.
///
/// `context` can be of "guilds" or "dms" (direct messages).
///
/// - `#[owners_only]`/`#[owners_only(bool)]`
/// Whether this command is exclusive to owners.
///
/// - `#[owner_privilege]`/`#[owner_privilege]
/// Whether this command has a privilege for owners (i.e certain options are ignored for them).
///
/// - `#[sub(commands)]`
/// A list of command names, separated by a comma, stating the subcommands of this command.
/// These are executed in the form: `this-command sub-command`
///
/// # Notes
/// The name of the command is parsed from the applied function,
/// or can be passed inside the `#[command]` attribute, a lá `#[command(foobar)]`.
///
/// This macro attribute generates static instances of `Command` and `CommandOptions`,
/// conserving the provided options.
///
/// The names of the instances are all uppercased names of the command name.
/// For example, with a name of "foo":
/// ```rust,ignore
/// pub static FOO_COMMAND_OPTIONS: CommandOptions = CommandOptions { ... };
/// pub static FOO_COMMAND: Command = Command { options: FOO_COMMAND_OPTIONS, ... };
/// ```
#[proc_macro_attribute]
pub fn command(attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let mut options = Options::default();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = match parse_values(attribute) {
            Ok(vals) => vals,
            Err(err) => return err.to_compile_error().into(),
        };

        let name = values.name.to_string();
        let name = &name[..];

        match name {
            "num_args" => {
                let mut args = 0;
                args.parse("num_args", values);

                options.min_args = Some(args);
                options.max_args = Some(args);
            }
            "required_permissions" => {
                let mut p = Vec::<Ident>::new();
                p.parse("required_permissions", values);

                let mut permissions = Permissions::default();
                for perm in p {
                    let p = match Permissions::from_str(&perm.to_string()) {
                        Some(p) => p,
                        None => {
                            return Error::new(perm.span(), "invalid permission")
                                .to_compile_error()
                                .into();
                        }
                    };

                    // Add them together.
                    permissions.0 |= p.0;
                }

                options.required_permissions = permissions;
            }
            "checks" => {
                let mut checks = Vec::<Ident>::new();
                checks.parse("checks", values);

                options.checks = Checks(checks);
            },
            "bucket" => {
                let mut buck = String::new();
                buck.parse("bucket", values);

                options.bucket = Some(buck);
            }
            "description" => {
                let mut desc = String::new();
                desc.parse("description", values);

                options.description = Some(desc);
            },
            "usage" => {
                let mut usage = String::new();
                usage.parse("usage", values);

                options.usage = Some(usage);
            },
            "example" => {
                let mut ex = String::new();
                ex.parse("example", values);

                options.example = Some(ex);
            },
            _ => {
                match_options!(name, values, options, span => [
                    min_args;
                    max_args;
                    aliases;
                    usage;
                    allowed_roles;
                    help_available;
                    only_in;
                    owners_only;
                    owner_privilege;
                    sub
                ]);
            }
        }
    }

    let Options {
        checks,
        bucket,
        aliases,
        description,
        usage,
        example,
        min_args,
        max_args,
        allowed_roles,
        required_permissions,
        help_available,
        only_in,
        owners_only,
        owner_privilege,
        sub,
    } = options;

    let description = AsOption(description);
    let usage = AsOption(usage);
    let bucket = AsOption(bucket);
    let example = AsOption(example);
    let min_args = AsOption(min_args);
    let max_args = AsOption(max_args);

    if let Err(err) = validate_declaration(&mut fun, false) {
        return err.to_compile_error().into();
    }

    if let Err(err) = validate_return_type(&mut fun) {
        return err.to_compile_error().into();
    }

    let name = _name.clone();

    // If name starts with a number, prepend an underscore to make it a valid identifier.
    let n = if _name.starts_with(|c: char| c.is_numeric()) {
        format!("_{}", _name)
    } else {
        _name
    };

    let _name = Ident::new(&n, Span::call_site());

    let Permissions(required_permissions) = required_permissions;

    let options = _name.with_suffix(COMMAND_OPTIONS);
    let sub = sub
        .into_iter()
        .map(|i| i.with_suffix(COMMAND))
        .collect::<Vec<_>>();

    let n = _name.with_suffix(COMMAND);
    let nn = fun.name.clone();

    let cfgs = fun.cfgs.clone();
    let cfgs2 = cfgs.clone();

    let options_path = quote!(serenity::framework::standard::CommandOptions);
    let command_path = quote!(serenity::framework::standard::Command);
    let permissions_path = quote!(serenity::model::permissions::Permissions);

    (quote! {
        #(#cfgs)*
        pub static #options: #options_path = #options_path {
            checks: #checks,
            bucket: #bucket,
            names: &[#name, #(#aliases),*],
            desc: #description,
            usage: #usage,
            example: #example,
            min_args: #min_args,
            max_args: #max_args,
            allowed_roles: &[#(#allowed_roles),*],
            required_permissions: #permissions_path { bits: #required_permissions },
            help_available: #help_available,
            only_in: #only_in,
            owners_only: #owners_only,
            owner_privilege: #owner_privilege,
            sub: &[#(&#sub),*],
        };

        #(#cfgs2)*
        pub static #n: #command_path = #command_path {
            fun: #nn,
            options: &#options,
        };

        #fun
    })
    .into()
}

/// A brother macro to [`command`], but for the help command.
/// An interface for simple browsing of all the available commands the bot provides,
/// and reading through specific information regarding a command.
///
/// As such, the options here will pertain in the help command's **layout** than its functionality.
///
/// # Options
///
/// - `#[suggestion_text(s)]`/`#[suggestion_text = s]`
/// For suggesting a command's name.
///
/// - `#[no_help_available_text(s)]`/`#[no_help_available_text = s]`
/// When help is unavailable for a command.
///
/// - `#[usage_label(s)]`/`#[usage_label = s]`
/// How should the command be used.
///
/// - `#[usage_sample_label(s)]`/`#[usage_sample_label = s]`
/// Actual sample label.
///
/// - `#[ungrouped_label(s)]`/`#[ungrouped_label = s]`
/// Ungrouped commands label.
///
/// - `#[description_label(s)]`/`#[description_label = s]`
/// Label at the start of the description.
///
/// - `#[grouped_label(s)]`/`#[grouped_label = s]`
/// Grouped commands label.
///
/// - `#[aliases_label(s)]`/`#[aliases_label = s]`
/// Label for a command's aliases.
///
/// - `#[guild_only_text(s)]`/`#[guild_only_text = s]`
/// When a command is specific to guilds only.
///
/// - `#[checks_label(s)]`/`#[checks_label = s]`
/// The header text when showing checks in the help command.
///
/// - `#[dm_only_text(s)]`/`#[dm_only_text = s]`
/// When a command is specific to dms only.
///
/// - `#[dm_and_guild_text(s)]`/`#[dm_guild_only_text = s]`
/// When a command is usable in both guilds and dms.
///
/// - `#[available_text(s)]`/`#[available_text = s]`
/// When a command is available.
///
/// - `#[command_not_found_text(s)]`/`#[command_not_found_text = s]`
/// When a command wasn't found.
///
/// - `#[individual_command_tip(s)]`/`#[individual_command_tip = s]`
/// How the user should access a command's details.
///
/// - `#[strikethrough_commands_tip_in_dm]`/`#[strikethrough_commands_tip_in_dm(s)]`/`#[strikethrough_commands_tip_in_dm = s]`
/// Reasoning behind strikethrough-commands.
///
/// If there wasn't any text passed, default text will be used instead.
///
/// *Only used in dms.*
///
/// - `#[strikethrough_commands_tip_in_guild]`/`#[strikethrough_commands_tip_in_guild(s)]`/`#[strikethrough_commands_tip_in_guild = s]`
/// Reasoning behind strikethrough-commands.
///
/// If there wasn't any text passed, default text will be used instead.
///
/// *Only used in guilds.*
///
/// - `#[group_prefix(s)]`/`#[group_prefix = s]`
/// For introducing a group's prefix
///
/// - `#[lacking_role(s)]`/`#[lacking_role = s]`
/// If a user lacks required roles, this will treat how commands will be displayed.
///
/// Accepts `strike` (strikethroughs), `hide` (will not be listed) or `nothing` (leave be).
///
/// - `#[lacking_permissions(s)]`/`#[lacking_role = s]`
/// If a user lacks permissions, this will treat how commands will be displayed.
///
/// Accepts `strike` (strikethroughs), `hide` (will not be listed) or `nothing` (leave be).
///
/// - `#[embed_error_colour(n)]`
/// Colour that the help-embed will use upon an error.
///
/// Value is a name to one of the provided constants of the `Colour` struct.
///
///- `#[embed_success_colour(n)]`
/// Colour that the help-embed will use normally.
///
/// Value is a name to one of the provided constants of the `Colour` struct.
///
/// - `#[max_levenshtein_distance(n)]`
/// How much should the help command search for a similiar name.
///
/// [`command`]: fn.command.html
#[proc_macro_attribute]
pub fn help(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let mut options = HelpOptions::default();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = match parse_values(attribute) {
            Ok(vals) => vals,
            Err(err) => return err.to_compile_error().into(),
        };

        let name = values.name.to_string();
        let name = &name[..];

        match name {
            "lacking_role" => {
                let mut behaviour = String::with_capacity(7);
                behaviour.parse("lacking_role", values);

                options.lacking_role = match HelpBehaviour::from_str(&behaviour) {
                    Some(h) => h,
                    None => {
                        return Error::new(
                            span,
                            &format!("invalid help behaviour: {:?}", behaviour),
                        )
                        .to_compile_error()
                        .into();
                    }
                };
            }
            "lacking_permissions" => {
                let mut behaviour = String::with_capacity(7);
                behaviour.parse("lacking_permissions", values);

                options.lacking_permissions = match HelpBehaviour::from_str(&behaviour) {
                    Some(h) => h,
                    None => {
                        return Error::new(
                            span,
                            &format!("invalid help behaviour: {:?}", behaviour),
                        )
                        .to_compile_error()
                        .into();
                    }
                };
            }
            "lacking_ownership" => {
                let mut behaviour = String::with_capacity(7);
                behaviour.parse("lacking_ownership", values);

                options.lacking_ownership = match HelpBehaviour::from_str(&behaviour) {
                    Some(h) => h,
                    None => {
                        return Error::new(
                            span,
                            &format!("invalid help behaviour: {:?}", behaviour),
                        )
                        .to_compile_error()
                        .into();
                    }
                };
            }
            "wrong_channel" => {
                let mut behaviour = String::with_capacity(7);
                behaviour.parse("wrong_channel", values);

                options.wrong_channel = match HelpBehaviour::from_str(&behaviour) {
                    Some(h) => h,
                    None => {
                        return Error::new(
                            span,
                            &format!("invalid help behaviour: {:?}", behaviour),
                        )
                        .to_compile_error()
                        .into();
                    }
                };
            }
            _ => {
                match_options!(name, values, options, span => [
                    suggestion_text;
                    no_help_available_text;
                    usage_label;
                    usage_sample_label;
                    ungrouped_label;
                    grouped_label;
                    aliases_label;
                    description_label;
                    guild_only_text;
                    checks_label;
                    dm_only_text;
                    dm_and_guild_text;
                    available_text;
                    command_not_found_text;
                    individual_command_tip;
                    group_prefix;
                    strikethrough_commands_tip_in_dm;
                    strikethrough_commands_tip_in_guild;
                    embed_error_colour;
                    embed_success_colour;
                    max_levenshtein_distance
                ]);
            }
        }
    }

    fn produce_strike_text(options: &HelpOptions, dm_or_guild: &str) -> Option<String> {
        use std::fmt::Write;

        let mut strike_text =
            String::from("~~`Strikethrough commands`~~ are unavailable because they");
        let mut is_any_option_strike = false;

        let mut concat_with_comma = if options.lacking_permissions == HelpBehaviour::Strike {
            is_any_option_strike = true;
            strike_text.push_str(" require permissions");

            true
        } else {
            false
        };

        if options.lacking_role == HelpBehaviour::Strike {
            is_any_option_strike = true;

            if concat_with_comma {
                strike_text.push_str(", require a specific role");
            } else {
                strike_text.push_str(" require a specific role");
                concat_with_comma = true;
            }
        }

        if options.wrong_channel == HelpBehaviour::Strike {
            is_any_option_strike = true;

            if concat_with_comma {
                let _ = write!(strike_text, ", or are limited to {}", dm_or_guild);
            } else {
                let _ = write!(strike_text, " are limited to {}", dm_or_guild);
            }
        }

        strike_text.push('.');

        if is_any_option_strike {
            Some(strike_text)
        } else {
            None
        }
    }

    if options.strikethrough_commands_tip_in_dm == Some(String::new()) {
        options.strikethrough_commands_tip_in_dm = produce_strike_text(&options, "direct messages");
    }

    if options.strikethrough_commands_tip_in_guild == Some(String::new()) {
        options.strikethrough_commands_tip_in_guild =
            produce_strike_text(&options, "guild messages");
    }

    let HelpOptions {
        suggestion_text,
        no_help_available_text,
        usage_label,
        usage_sample_label,
        ungrouped_label,
        grouped_label,
        aliases_label,
        description_label,
        guild_only_text,
        checks_label,
        dm_only_text,
        dm_and_guild_text,
        available_text,
        command_not_found_text,
        individual_command_tip,
        group_prefix,
        strikethrough_commands_tip_in_dm,
        strikethrough_commands_tip_in_guild,
        lacking_role,
        lacking_permissions,
        lacking_ownership,
        wrong_channel,
        embed_error_colour,
        embed_success_colour,
        max_levenshtein_distance,
    } = options;

    let strikethrough_commands_tip_in_dm = AsOption(strikethrough_commands_tip_in_dm);
    let strikethrough_commands_tip_in_guild = AsOption(strikethrough_commands_tip_in_guild);

    if let Err(err) = validate_declaration(&mut fun, true) {
        return err.to_compile_error().into();
    }

    if let Err(err) = validate_return_type(&mut fun) {
        return err.to_compile_error().into();
    }

    let options = fun.name.with_suffix(HELP_OPTIONS);

    let n = fun.name.with_suffix(HELP);
    let nn = fun.name.clone();

    let cfgs = fun.cfgs.clone();
    let cfgs2 = cfgs.clone();

    let options_path = quote!(serenity::framework::standard::HelpOptions);
    let command_path = quote!(serenity::framework::standard::HelpCommand);
    let colour_path = quote!(serenity::utils::Colour);

    (quote! {
        #(#cfgs)*
        pub static #options: #options_path = #options_path {
            suggestion_text: #suggestion_text,
            no_help_available_text: #no_help_available_text,
            usage_label: #usage_label,
            usage_sample_label: #usage_sample_label,
            ungrouped_label: #ungrouped_label,
            grouped_label: #grouped_label,
            aliases_label: #aliases_label,
            description_label: #description_label,
            guild_only_text: #guild_only_text,
            checks_label: #checks_label,
            dm_only_text: #dm_only_text,
            dm_and_guild_text: #dm_and_guild_text,
            available_text: #available_text,
            command_not_found_text: #command_not_found_text,
            individual_command_tip: #individual_command_tip,
            group_prefix: #group_prefix,
            strikethrough_commands_tip_in_dm: #strikethrough_commands_tip_in_dm,
            strikethrough_commands_tip_in_guild: #strikethrough_commands_tip_in_guild,
            lacking_role: #lacking_role,
            lacking_permissions: #lacking_permissions,
            lacking_ownership: #lacking_ownership,
            wrong_channel: #wrong_channel,
            embed_error_colour: #colour_path(#embed_error_colour),
            embed_success_colour: #colour_path(#embed_success_colour),
            max_levenshtein_distance: #max_levenshtein_distance,
        };

        #(#cfgs2)*
        pub static #n: #command_path = #command_path {
            fun: #nn,
            options: &#options,
        };

        #fun
    })
    .into()
}

/// Create a grouping of commands.
///
/// It is a prerequisite for all commands to be assigned under a common group,
/// before they may be executed by a user.
///
/// A group might have one or more *prefixes* set. This will necessitate for
/// one of the prefixes to appear before the group's command.
/// For example, for a general prefix `!`, a group prefix `foo` and a command `bar`,
/// the invocation would look like this: `!foo bar`.
///
/// It might have some options apply to *all* of its commands. E.g. guild or dm only.
///
/// Its options may be *inherited* from another group.
///
/// It may even couple other groups as well.
///
/// This group macro purports all of the said purposes above, in a json-like syntax:
///
/// ```rust,no_run
/// use command_attr::{command, group};
///
/// #[command]
/// fn bar() {
///     println!("baz");
/// }
///
/// group!({
///     name: "baz",
///     options: {
///         prefixes: ["baz"] // Any sub-group **must** have a prefix.
///     },
///     commands: [],
/// });
///
/// group!({
///     name: "foo",
///     options: {
///         prefixes: ["foo"],
///     },
///     commands: [bar],
///     sub: [baz],
/// });
/// ```
///
/// # Options
///
/// These appear inside the object of the `options` field:
///
/// - `prefixes`: Array<String>
/// The group's prefixes.
///
/// - `allowed_roles`: Array<String>
/// Only which roles may execute this group's commands.
///
/// - `only`: String
/// Whether this group's commands are restricted to `guilds` or `dms`.
///
/// - `owner_only`: Bool
/// If only the owners of the bot may execute this group's commands.
///
/// - `owner_privilege`: Bool
/// Whether the owners should be treated as normal users.
///
/// Default value is `true`
///
/// - `help_available`: bool
/// Whether the group is visible to the help command.
///
/// Default value is `true`
///
/// - `checks`: Array<Ident>
/// A set of preconditions that must be met before a group command's execution.
/// Refer to [`command`]'s `checks` documentation.
///
/// - `required_permissions`: Array<Ident>
/// A set of permissions needed by the user before a group command's execution.
///
/// - `default_command`: Ident
/// Command to be executed if none of the group's prefixes are given.
/// Identifier must refer to a `#[command]`'d function.
///
/// - `prefix`: String
/// Assign a single prefix to this group.
///
/// - `description`: String
/// The description of the group.
/// Used in the help command.
///
/// - `inherit`: Access
/// Derive options from another `GroupOptions` instance.
///
/// On standalone `GroupOptions`: `$name_of_options$`
/// `GroupOptions` belonging to another `Group`: `$name_of_group$.options`
///
/// [`command`]: #fn.command.html
#[proc_macro]
pub fn group(input: TokenStream) -> TokenStream {
    let group = parse_macro_input!(input as Group);

    group.into_token_stream().into()
}

/// Create an instance of `GroupOptions`.
/// Useful when making default options and then deriving them for command groups.
///
/// ```rust,no_run
/// use command_attr::group_options;
///
/// // First argument is the name of the options; second the actual options.
/// group_options!("foobar", {
///     description: "I'm an example group",
/// });
/// ```
#[proc_macro]
pub fn group_options(input: TokenStream) -> TokenStream {
    struct GroupOptionsName {
        name: Ident,
        options: GroupOptions,
    }

    impl Parse for GroupOptionsName {
        fn parse(input: ParseStream) -> Result<Self> {
            let name = input.parse::<Ident>()?;

            let options = input.parse::<GroupOptions>()?;

            Ok(GroupOptionsName { name, options })
        }
    }

    let GroupOptionsName { name, options } = parse_macro_input!(input as GroupOptionsName);

    let name = name.with_suffix(GROUP_OPTIONS);

    let options_path = quote!(serenity::framework::standard::GroupOptions);

    (quote! {
        pub static #name: #options_path = #options;
    })
    .into()
}