--- @meta confit.config

--- Document collector threading into the profile return.
--- @class Config
local _CLASS_Config_ = {
	--- Records one document under its destination with its body.
	--- @param self Config
	--- @param dest PathHandle
	--- @param body TextBody | StructuredBody
	add_document = function(self, dest, body) end,
}

--- @alias SerializableValue nil | boolean | integer | number | string | table

--- @alias TextBody { text: string }

--- @alias StructuredBody { data: { [string]: SerializableValue }, format: string }

--- Builds one named collector.
--- @param name string Collector name.
--- @return Config #1: The collector threading into the profile return.
function config(name) end


return config
